//! The tabs as the socket thread keeps them: what the engine asks turned into DevTools calls, and
//! what Chromium reports turned into [`Event`]s.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use base64::Engine as _;
use serde_json::{Value, json};

use super::cdp::Wire;
use super::{Event, TabId};

/// What the engine asks of the socket thread.
pub(super) enum Command {
    /// Opens a tab on this address.
    Open(String),
    /// Closes the tab.
    Close(TabId),
    /// Calls a method on the tab and does not wait for its answer.
    Call(TabId, &'static str, Value),
    /// Goes this many steps through the tab's history: -1 back, 1 forward.
    History(TabId, i64),
    /// Gives the tab this viewport, in CSS pixels.
    Viewport(TabId, u32, u32),
    /// Makes this the tab whose frames flow.
    Show(TabId),
    /// Runs the expression in the tab and answers with its value.
    Evaluate(TabId, String, Sender<Result<Value, String>>),
    /// Asks the browser to close.
    Quit,
}

/// What a sent call's answer is for.
enum Pending {
    /// The tab's page events are on.
    Enabled(String),
    /// The history, to report where the tab is now.
    Where(String),
    /// The history, to step through it.
    Step(String, i64),
    /// A value someone waits for.
    Evaluate(Sender<Result<Value, String>>),
}

/// One tab Chromium has and this thread is attached to.
struct Tab {
    session: String,
    title: String,
    url: String,
    loading: bool,
    /// Where the tab was last reported to be: address, can go back, can go forward.
    reported: Option<(String, bool, bool)>,
    viewport: Option<(u32, u32)>,
    crashed: bool,
}

/// Every tab, keyed by target id, and what is needed to follow them.
pub(super) struct Tabs {
    events: Sender<Event>,
    tabs: HashMap<String, Tab>,
    /// Session id to target id.
    sessions: HashMap<String, String>,
    /// Targets an attach was asked for, with the tab that opened them, if any.
    attaching: HashMap<String, Option<String>>,
    pending: HashMap<u64, Pending>,
    shown: Option<String>,
}

impl Tabs {
    pub(super) fn new(events: Sender<Event>) -> Self {
        Self {
            events,
            tabs: HashMap::new(),
            sessions: HashMap::new(),
            attaching: HashMap::new(),
            pending: HashMap::new(),
            shown: None,
        }
    }

    /// Hands over an event; nobody listening any more is not this thread's concern.
    pub(super) fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    /// Carries out one command.
    pub(super) fn command(&mut self, wire: &mut Wire, command: Command) {
        match command {
            Command::Open(url) => {
                wire.call(None, "Target.createTarget", json!({ "url": url }));
            }
            Command::Close(tab) => {
                wire.call(None, "Target.closeTarget", json!({ "targetId": tab.0 }));
            }
            Command::Call(tab, method, params) => {
                if let Some(session) = self.session(&tab) {
                    wire.call(Some(&session), method, params);
                }
            }
            Command::History(tab, step) => {
                if let Some(session) = self.session(&tab) {
                    let id = wire.call(Some(&session), "Page.getNavigationHistory", json!({}));
                    self.pending.insert(id, Pending::Step(tab.0, step));
                }
            }
            Command::Viewport(tab, width, height) => {
                let Some(state) = self.tabs.get_mut(&tab.0) else { return };
                state.viewport = Some((width, height));
                let session = state.session.clone();
                wire.call(
                    Some(&session),
                    "Emulation.setDeviceMetricsOverride",
                    json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": false }),
                );
                if self.shown.as_ref() == Some(&tab.0) {
                    // Restarted so the frames take the new size at once.
                    wire.call(Some(&session), "Page.stopScreencast", json!({}));
                    self.start_screencast(wire, &tab.0);
                }
            }
            Command::Show(tab) => {
                if self.shown.as_ref() == Some(&tab.0) {
                    return;
                }
                if let Some(session) = self.shown.take().and_then(|old| self.tabs.get(&old)).map(|t| t.session.clone())
                {
                    wire.call(Some(&session), "Page.stopScreencast", json!({}));
                }
                self.start_screencast(wire, &tab.0);
                // Kept even for a tab not attached yet: its frames start once it is.
                self.shown = Some(tab.0);
            }
            Command::Evaluate(tab, expression, reply) => match self.session(&tab) {
                Some(session) => {
                    let id = wire.call(
                        Some(&session),
                        "Runtime.evaluate",
                        json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
                    );
                    self.pending.insert(id, Pending::Evaluate(reply));
                }
                None => {
                    let _ = reply.send(Err(format!("no tab {}", tab.0)));
                }
            },
            Command::Quit => {
                wire.call(None, "Browser.close", json!({}));
            }
        }
    }

    fn session(&self, tab: &TabId) -> Option<String> {
        self.tabs.get(&tab.0).map(|state| state.session.clone())
    }

    fn start_screencast(&self, wire: &mut Wire, target: &str) {
        let Some(tab) = self.tabs.get(target) else { return };
        let mut params = json!({ "format": "jpeg", "quality": 80, "everyNthFrame": 1 });
        if let Some((width, height)) = tab.viewport {
            params["maxWidth"] = json!(width);
            params["maxHeight"] = json!(height);
        }
        wire.call(Some(&tab.session), "Page.startScreencast", params);
    }

    /// Asks for the tab's history, to report where it is.
    fn ask_where(&mut self, wire: &mut Wire, target: &str) {
        if let Some(tab) = self.tabs.get(target) {
            let id = wire.call(Some(&tab.session), "Page.getNavigationHistory", json!({}));
            self.pending.insert(id, Pending::Where(target.to_owned()));
        }
    }

    /// Handles one message from Chromium: a call's answer or an event.
    pub(super) fn message(&mut self, wire: &mut Wire, message: &Value) {
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            if let Some(pending) = self.pending.remove(&id) {
                self.answer(wire, pending, message);
            }
            return;
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else { return };
        let params = message.get("params").unwrap_or(&Value::Null);
        let session = message.get("sessionId").and_then(Value::as_str);
        let target = session.and_then(|session| self.sessions.get(session)).cloned();
        match (method, target) {
            ("Target.targetCreated", _) => self.created(wire, &params["targetInfo"]),
            ("Target.attachedToTarget", _) => self.attached(wire, params),
            ("Target.targetInfoChanged", _) => self.info_changed(wire, &params["targetInfo"]),
            ("Target.targetDestroyed", _) => self.gone(params["targetId"].as_str().unwrap_or_default()),
            ("Target.detachedFromTarget", _) => {
                let target = params["sessionId"].as_str().and_then(|session| self.sessions.get(session)).cloned();
                self.gone(&target.unwrap_or_default());
            }
            ("Target.targetCrashed", _) => self.crashed(params["targetId"].as_str().unwrap_or_default()),
            ("Inspector.targetCrashed", Some(target)) => self.crashed(&target),
            ("Page.frameNavigated", Some(target)) if params["frame"].get("parentId").is_none() => {
                if let Some(tab) = self.tabs.get_mut(&target) {
                    tab.crashed = false;
                }
                self.ask_where(wire, &target);
            }
            ("Page.navigatedWithinDocument", Some(target)) if params["frameId"].as_str() == Some(&target) => {
                self.ask_where(wire, &target);
            }
            ("Page.frameStartedLoading", Some(target)) => self.loading(&target, params, true),
            ("Page.frameStoppedLoading", Some(target)) => self.loading(&target, params, false),
            ("Page.screencastFrame", Some(target)) => self.frame(wire, &target, params),
            ("Runtime.bindingCalled", Some(target)) if params["name"] == TITLE_BINDING => {
                self.titled(&target, params["payload"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }

    fn answer(&mut self, wire: &mut Wire, pending: Pending, message: &Value) {
        let result = &message["result"];
        match pending {
            Pending::Enabled(target) => self.ask_where(wire, &target),
            Pending::Evaluate(reply) => {
                let value = if let Some(error) = message.get("error") {
                    Err(error["message"].as_str().unwrap_or("the call failed").to_owned())
                } else if let Some(exception) = result.get("exceptionDetails") {
                    let text = exception["exception"]["description"].as_str().or_else(|| exception["text"].as_str());
                    Err(text.unwrap_or("the expression threw").to_owned())
                } else {
                    Ok(result["result"].get("value").cloned().unwrap_or(Value::Null))
                };
                let _ = reply.send(value);
            }
            Pending::Where(target) => {
                let Some((current, entries)) = history(result) else { return };
                let Some(tab) = self.tabs.get_mut(&target) else { return };
                let url = entries[current]["url"].as_str().unwrap_or_default().to_owned();
                let now = (url, current > 0, current + 1 < entries.len());
                if tab.reported.as_ref() == Some(&now) {
                    return;
                }
                tab.reported = Some(now.clone());
                let (url, can_back, can_forward) = now;
                self.emit(Event::Navigated { tab: TabId(target), url, can_back, can_forward });
            }
            Pending::Step(target, step) => {
                let Some((current, entries)) = history(result) else { return };
                let Some(index) = current.checked_add_signed(isize::try_from(step).unwrap_or(0)) else { return };
                let (Some(entry), Some(session)) = (entries.get(index), self.session(&TabId(target))) else { return };
                wire.call(Some(&session), "Page.navigateToHistoryEntry", json!({ "entryId": entry["id"] }));
            }
        }
    }

    /// A target appeared: a page is attached to, whoever opened it.
    fn created(&mut self, wire: &mut Wire, info: &Value) {
        let Some(target) = info["targetId"].as_str() else { return };
        if info["type"] != "page" || self.tabs.contains_key(target) || self.attaching.contains_key(target) {
            return;
        }
        let opener = info["openerId"].as_str().filter(|opener| self.tabs.contains_key(*opener)).map(str::to_owned);
        self.attaching.insert(target.to_owned(), opener);
        wire.call(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }));
    }

    /// A page is attached: its events are switched on and it becomes a tab.
    fn attached(&mut self, wire: &mut Wire, params: &Value) {
        let info = &params["targetInfo"];
        let (Some(session), Some(target)) = (params["sessionId"].as_str(), info["targetId"].as_str()) else { return };
        let Some(opener) = self.attaching.remove(target) else { return };
        let enabled = wire.call(Some(session), "Page.enable", json!({}));
        // The page may arrive before its events are switched on, and then no navigation is ever
        // heard of; once they are on, where it is is asked so that it is reported in any case.
        self.pending.insert(enabled, Pending::Enabled(target.to_owned()));
        wire.call(Some(session), "Inspector.enable", json!({}));
        watch_title(wire, session);
        let url = info["url"].as_str().unwrap_or_default().to_owned();
        self.sessions.insert(session.to_owned(), target.to_owned());
        self.tabs.insert(
            target.to_owned(),
            Tab {
                session: session.to_owned(),
                // Chromium fills in the address while a page has no title; the watcher reports the real one.
                title: String::new(),
                url: url.clone(),
                loading: false,
                reported: None,
                viewport: None,
                crashed: false,
            },
        );
        if self.shown.as_deref() == Some(target) {
            self.start_screencast(wire, target);
        }
        self.emit(Event::TabOpened { tab: TabId(target.to_owned()), opener: opener.map(TabId), url });
    }

    fn info_changed(&mut self, wire: &mut Wire, info: &Value) {
        let Some(target) = info["targetId"].as_str() else { return };
        let Some(tab) = self.tabs.get_mut(target) else { return };
        let url = info["url"].as_str().unwrap_or_default();
        if tab.url != url {
            tab.url = url.to_owned();
            self.ask_where(wire, target);
        }
    }

    fn titled(&mut self, target: &str, title: &str) {
        let Some(tab) = self.tabs.get_mut(target) else { return };
        if tab.title != title {
            title.clone_into(&mut tab.title);
            self.emit(Event::Title { tab: TabId(target.to_owned()), title: title.to_owned() });
        }
    }

    fn gone(&mut self, target: &str) {
        self.attaching.remove(target);
        let Some(tab) = self.tabs.remove(target) else { return };
        self.sessions.remove(&tab.session);
        if self.shown.as_deref() == Some(target) {
            self.shown = None;
        }
        self.emit(Event::TabClosed { tab: TabId(target.to_owned()) });
    }

    /// The tab's renderer died. Both the browser and the tab's own session report it; the tab
    /// hears of it once.
    fn crashed(&mut self, target: &str) {
        let Some(tab) = self.tabs.get_mut(target) else { return };
        if tab.crashed {
            return;
        }
        tab.crashed = true;
        tab.loading = false;
        self.emit(Event::Crashed { tab: TabId(target.to_owned()) });
    }

    /// The main frame's loading began or ended; a page's main frame has the target's id.
    fn loading(&mut self, target: &str, params: &Value, loading: bool) {
        if params["frameId"].as_str() != Some(target) {
            return;
        }
        let Some(tab) = self.tabs.get_mut(target) else { return };
        if tab.loading != loading {
            tab.loading = loading;
            self.emit(Event::Loading { tab: TabId(target.to_owned()), loading });
        }
    }

    /// A frame arrived: acknowledged at once so the next one comes, and handed over when its tab
    /// is still the one shown.
    fn frame(&mut self, wire: &mut Wire, target: &str, params: &Value) {
        let Some(tab) = self.tabs.get(target) else { return };
        wire.call(Some(&tab.session), "Page.screencastFrameAck", json!({ "sessionId": params["sessionId"] }));
        if self.shown.as_deref() != Some(target) {
            return;
        }
        let Some(data) = params["data"].as_str() else { return };
        if let Ok(jpeg) = base64::engine::general_purpose::STANDARD.decode(data) {
            self.emit(Event::Frame { tab: TabId(target.to_owned()), jpeg });
        }
    }

    /// Tells every caller still waiting that no answer will come.
    pub(super) fn abandon(&mut self) {
        for (_, pending) in self.pending.drain() {
            if let Pending::Evaluate(reply) = pending {
                let _ = reply.send(Err("Chromium is gone".to_owned()));
            }
        }
    }
}

/// The name of the isolated world the title watcher runs in. The page's own scripts cannot see
/// it or the binding it reports through.
const WORLD: &str = "qbrowser";

/// The function the title watcher calls with each new title.
const TITLE_BINDING: &str = "qbrowserTitle";

/// Reports the document's title whenever it changes. A document that is still being read has no
/// title yet rather than an empty one, so none is reported before it is read unless one appears.
const TITLE_WATCHER: &str = "(() => {
    let last = null;
    const report = () => {
        if (document.title === '' && document.readyState === 'loading') {
            return;
        }
        if (document.title !== last) {
            last = document.title;
            qbrowserTitle(last);
        }
    };
    new MutationObserver(report).observe(document, { subtree: true, childList: true, characterData: true });
    document.addEventListener('DOMContentLoaded', report);
    report();
})();";

/// Has every document of the session report its title. Chromium's own target info carries a
/// title too, but only picks up changes on navigation: a title a script sets later (a count of
/// unread mail) would never be heard of.
fn watch_title(wire: &mut Wire, session: &str) {
    // Bindings only report while the runtime's events are on.
    wire.call(Some(session), "Runtime.enable", json!({}));
    wire.call(Some(session), "Runtime.addBinding", json!({ "name": TITLE_BINDING, "executionContextName": WORLD }));
    wire.call(
        Some(session),
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": TITLE_WATCHER, "worldName": WORLD, "runImmediately": true }),
    );
}

/// The current index and the entries of a `Page.getNavigationHistory` result.
fn history(result: &Value) -> Option<(usize, &Vec<Value>)> {
    let entries = result["entries"].as_array()?;
    let current = usize::try_from(result["currentIndex"].as_u64()?).ok()?;
    (current < entries.len()).then_some((current, entries))
}
