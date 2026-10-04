//! The tabs as the socket thread keeps them: what the engine asks turned into DevTools calls, and
//! what Chromium reports turned into [`Event`]s.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use base64::Engine as _;
use serde_json::{Value, json};

use super::cdp::Wire;
use super::history::Steps;
use super::reader;
use super::{DialogKind, Entry, Event, Reading, TabId};

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
    /// Gives the tab this viewport, in the page area's own pixels.
    Viewport(TabId, u32, u32),
    /// Draws the tab's page at this many per cent of its size.
    Zoom(TabId, u32),
    /// Makes this the tab whose frames flow.
    Show(TabId),
    /// Keeps every frame within this many pixels, or lets it be the page area's full size.
    PictureLimit(Option<(u32, u32)>),
    /// Runs the expression in the tab and answers with its value.
    Evaluate(TabId, String, Sender<Result<Value, String>>),
    /// Runs the expression in the tab and answers with [`Event::Answered`], without waiting.
    Ask(TabId, String),
    /// Reads the tab's page where the page cannot see and answers with what it says.
    Read(TabId, Sender<Result<Reading, String>>),
    /// Runs the expression in the tab's isolated world and answers with its value.
    EvaluateIsolated(TabId, String, Sender<Result<Value, String>>),
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
    /// A value the tab's page was asked for, to be reported as an event.
    Ask(TabId),
    /// The world the page is read in, to run the reading script in it.
    World { session: String, reply: Sender<Result<Reading, String>> },
    /// The script's answer, to be read into a [`Reading`].
    Reading(Sender<Result<Reading, String>>),
}

/// The per cent of its own size a tab's page is drawn at until the screen asks for another: the
/// size the page area already is, so a tab opens with nothing said about it.
pub(crate) const HOME: u32 = 100;

/// The narrowest and widest per cent the page may be drawn at: the ends of the ladder the screen
/// asks from (`crate::app::zoom::LADDER`). A level outside them is brought inside rather than
/// handed to Chromium, which would lay the page out for a picture that cannot carry it.
pub(crate) const ZOOM_BOUNDS: (u32, u32) = (30, 200);

/// The CSS pixels a page area of `pixels` is laid out for at `percent`: the area's own size
/// divided by the zoom, rounded **down** to a whole CSS pixel and never to nothing.
///
/// Down and never up: a CSS size one pixel over would make the frame a pixel wider than the area
/// it is drawn in, and the picture would be scaled down again by that pixel, which is the very
/// thing the zoom is there to undo.
pub(crate) fn css_pixels(pixels: u32, percent: u32) -> u32 {
    let zoom = u64::from(percent.clamp(ZOOM_BOUNDS.0, ZOOM_BOUNDS.1));
    // The multiplication is in u64 so a large area times a hundred cannot wrap before the
    // division, and the whole number is kept in u32 as the protocol asks for it.
    let whole = u64::from(pixels) * 100 / zoom;
    u32::try_from(whole).unwrap_or(u32::MAX).max(1)
}

/// One tab Chromium has and this thread is attached to.
struct Tab {
    session: String,
    title: String,
    /// The text the page last reported as selected; empty when it has no selection.
    selection: String,
    url: String,
    loading: bool,
    /// Where the tab was last reported to be: address, can go back, can go forward.
    reported: Option<(String, bool, bool)>,
    /// The tab's whole history last reported: which step it is at and every step. It sits beside
    /// `reported` because both are what was last said about this tab, and keeping it is how the
    /// same answer is recognised and not told twice.
    steps: Option<(usize, Vec<Entry>)>,
    /// The page area's own pixel size, from which the CSS viewport is worked out at every zoom.
    viewport: Option<(u32, u32)>,
    /// The per cent of its size the page is drawn at, [`HOME`] until the screen asks otherwise.
    zoom: u32,
    crashed: bool,
    /// The execution context of the isolated world in the tab's main frame, once Chromium has
    /// said which one it is; a new document makes a new one.
    world: Option<i64>,
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
    /// The largest frame the screen can use, whatever the page area's pixels; see
    /// [`Engine::set_picture_limit`](super::Engine::set_picture_limit).
    picture_limit: Option<(u32, u32)>,
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
            picture_limit: None,
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
                self.lay_out(wire, &tab.0);
            }
            Command::Zoom(tab, percent) => {
                let Some(state) = self.tabs.get_mut(&tab.0) else { return };
                // A level the ladder does not carry is brought inside it rather than refused, and
                // the caller sees which level is in force in what the tab is then drawn at.
                state.zoom = percent.clamp(ZOOM_BOUNDS.0, ZOOM_BOUNDS.1);
                self.lay_out(wire, &tab.0);
            }
            Command::PictureLimit(limit) => {
                if self.picture_limit == limit {
                    return;
                }
                self.picture_limit = limit;
                if let Some(shown) = self.shown.clone()
                    && let Some(session) = self.tabs.get(&shown).map(|tab| tab.session.clone())
                {
                    wire.call(Some(&session), "Page.stopScreencast", json!({}));
                    self.start_screencast(wire, &shown);
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
            Command::Ask(tab, expression) => {
                let Some(session) = self.session(&tab) else {
                    let value = Err(format!("no tab {}", tab.0));
                    self.emit(Event::Answered { tab, value });
                    return;
                };
                let id = wire.call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
                );
                self.pending.insert(id, Pending::Ask(tab));
            }
            Command::Read(tab, reply) => match self.session(&tab) {
                Some(session) => {
                    // A world of the tab's own that the page's scripts are not in, so nothing it
                    // wrote can be seen while its page is read and nothing it does can be.
                    let id = wire.call(
                        Some(&session),
                        "Page.createIsolatedWorld",
                        json!({ "frameId": tab.0.as_str(), "worldName": WORLD }),
                    );
                    self.pending.insert(id, Pending::World { session, reply });
                }
                None => {
                    let _ = reply.send(Err(format!("no tab {}", tab.0)));
                }
            },
            Command::EvaluateIsolated(tab, expression, reply) => {
                // The page's own scripts live in the main world and cannot see this one, so what
                // is asked here can neither be watched nor answered falsely by them.
                let Some((session, Some(world))) =
                    self.tabs.get(&tab.0).map(|state| (state.session.clone(), state.world))
                else {
                    let _ = reply.send(Err(format!("no isolated world in tab {}", tab.0)));
                    return;
                };
                let id = wire.call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({ "expression": expression, "contextId": world, "returnByValue": true, "awaitPromise": true }),
                );
                self.pending.insert(id, Pending::Evaluate(reply));
            }
            Command::Quit => {
                wire.call(None, "Browser.close", json!({}));
            }
        }
    }

    fn session(&self, tab: &TabId) -> Option<String> {
        self.tabs.get(&tab.0).map(|state| state.session.clone())
    }

    /// Tells the page the viewport its own pixel size and zoom together make, and takes the frames
    /// over again, so a shown tab's next picture has the new size at once instead of waiting for
    /// the page to repaint itself.
    fn lay_out(&self, wire: &mut Wire, target: &str) {
        let Some(tab) = self.tabs.get(target) else { return };
        let session = tab.session.clone();
        if let Some((width, height)) = tab.viewport {
            // A browser's zoom is a smaller CSS viewport at the same picture size: the page lays
            // out as if the window were narrower and the result is drawn at the size it had before,
            // so the writing is bigger and the layout has reflowed.
            //
            // `Emulation.setPageScaleFactor` would magnify without reflowing, which is a
            // pinch-zoom on a phone rather than what a person means by zoom on a desktop, and its
            // effect ends on the next navigation; it is not used.
            wire.call(
                Some(&session),
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": css_pixels(width, tab.zoom),
                    "height": css_pixels(height, tab.zoom),
                    "deviceScaleFactor": f64::from(tab.zoom) / 100.0,
                    "mobile": false,
                }),
            );
        }
        if self.shown.as_deref() == Some(target) {
            // Restarted so the frames take the new size at once.
            wire.call(Some(&session), "Page.stopScreencast", json!({}));
            self.start_screencast(wire, target);
        }
    }

    fn start_screencast(&self, wire: &mut Wire, target: &str) {
        let Some(tab) = self.tabs.get(target) else { return };
        // Headless Chromium paints only the page in front. A tab opened behind another one (the
        // tabs of the last run, opened all at once) would otherwise never send a picture until
        // something on it changed.
        wire.call(Some(&tab.session), "Page.bringToFront", json!({}));
        let mut params = json!({ "format": "jpeg", "quality": 80, "everyNthFrame": 1 });
        if let Some((width, height)) = tab.viewport {
            // Chromium keeps the page's shape inside both bounds, so the smaller of the two sizes
            // holds on each side.
            let (width, height) = self.picture_limit.map_or((width, height), |(most_wide, most_tall)| {
                (width.min(most_wide).max(1), height.min(most_tall).max(1))
            });
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
            ("Page.javascriptDialogOpening", Some(target)) => {
                let kind = match params["type"].as_str() {
                    Some("confirm") => DialogKind::Confirm,
                    Some("prompt") => DialogKind::Prompt,
                    Some("beforeunload") => DialogKind::BeforeUnload,
                    _ => DialogKind::Alert,
                };
                let text = |key: &str| params[key].as_str().unwrap_or_default().to_owned();
                self.emit(Event::Dialog {
                    tab: TabId(target),
                    kind,
                    message: text("message"),
                    default_text: text("defaultPrompt"),
                });
            }
            ("Page.javascriptDialogClosed", Some(target)) => self.emit(Event::DialogClosed { tab: TabId(target) }),
            ("Runtime.executionContextCreated", Some(target))
                if params["context"]["name"] == WORLD
                    && params["context"]["auxData"]["frameId"].as_str() == Some(&target) =>
            {
                if let Some(tab) = self.tabs.get_mut(&target) {
                    tab.world = params["context"]["id"].as_i64();
                }
            }
            ("Runtime.executionContextDestroyed", Some(target)) => {
                if let Some(tab) = self.tabs.get_mut(&target)
                    && tab.world.is_some()
                    && tab.world == params["executionContextId"].as_i64()
                {
                    tab.world = None;
                }
            }
            ("Runtime.executionContextsCleared", Some(target)) => {
                if let Some(tab) = self.tabs.get_mut(&target) {
                    tab.world = None;
                }
            }
            ("Runtime.bindingCalled", Some(target)) if params["name"] == TITLE_BINDING => {
                self.titled(&target, params["payload"].as_str().unwrap_or_default());
            }
            ("Runtime.bindingCalled", Some(target)) if params["name"] == SELECTION_BINDING => {
                self.selected(&target, params["payload"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }

    fn answer(&mut self, wire: &mut Wire, pending: Pending, message: &Value) {
        let result = &message["result"];
        match pending {
            Pending::Enabled(target) => self.ask_where(wire, &target),
            Pending::Evaluate(reply) => {
                let _ = reply.send(value_of(message));
            }
            Pending::Ask(tab) => {
                let value = value_of(message);
                self.emit(Event::Answered { tab, value });
            }
            Pending::World { session, reply } => {
                let Some(context) = message["result"]["executionContextId"].as_u64() else {
                    let _ = reply.send(Err("the page has no world of its own to read in".to_owned()));
                    return;
                };
                let id = wire.call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({ "expression": reader::SCRIPT, "returnByValue": true, "contextId": context }),
                );
                self.pending.insert(id, Pending::Reading(reply));
            }
            Pending::Reading(reply) => {
                let _ = reply.send(value_of(message).and_then(|value| reader::reading(&value)));
            }
            Pending::Where(target) => {
                let Some(steps) = Steps::read(result) else { return };
                let whole = (steps.current(), steps.entries().to_vec());
                let Some(entry) = steps.entries().get(whole.0) else { return };
                let at = (entry.url.clone(), whole.0 > 0, whole.0 + 1 < whole.1.len());
                for event in self.heard(target, at, whole) {
                    self.emit(event);
                }
            }
            Pending::Step(target, step) => {
                let Some(steps) = Steps::read(result) else { return };
                let Some(index) = steps.current().checked_add_signed(isize::try_from(step).unwrap_or(0)) else {
                    return;
                };
                let (Some(entry), Some(session)) = (steps.id(index), self.session(&TabId(target))) else { return };
                wire.call(Some(&session), "Page.navigateToHistoryEntry", json!({ "entryId": entry }));
            }
        }
    }

    /// Records what one `Page.getNavigationHistory` answer says about a tab, and gives back the
    /// parts of it that are new, in the order they are heard: where the tab is, and then the whole
    /// list it walked to get there. An answer that says what the last one said is not heard again.
    fn heard(&mut self, target: String, at: (String, bool, bool), whole: (usize, Vec<Entry>)) -> Vec<Event> {
        let Some(tab) = self.tabs.get_mut(&target) else { return Vec::new() };
        let mut events = Vec::new();
        let id = TabId(target);
        if tab.reported.as_ref() != Some(&at) {
            tab.reported = Some(at.clone());
            events.push(Event::Navigated { tab: id.clone(), url: at.0, can_back: at.1, can_forward: at.2 });
        }
        if tab.steps.as_ref() != Some(&whole) {
            tab.steps = Some(whole.clone());
            events.push(Event::History { tab: id, current: whole.0, entries: whole.1 });
        }
        events
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
        watch_selection(wire, session);
        let url = info["url"].as_str().unwrap_or_default().to_owned();
        self.sessions.insert(session.to_owned(), target.to_owned());
        self.tabs.insert(
            target.to_owned(),
            Tab {
                session: session.to_owned(),
                // Chromium fills in the address while a page has no title; the watcher reports the real one.
                title: String::new(),
                selection: String::new(),
                url: url.clone(),
                loading: false,
                reported: None,
                steps: None,
                viewport: None,
                zoom: HOME,
                crashed: false,
                world: None,
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

    /// The page's selection changed, and what it holds now is reported even when that is nothing,
    /// so a selection the person has let go of cannot stay on the tab.
    fn selected(&mut self, target: &str, text: &str) {
        let Some(tab) = self.tabs.get_mut(target) else { return };
        if tab.selection != text {
            text.clone_into(&mut tab.selection);
            self.emit(Event::Selection { tab: TabId(target.to_owned()), text: text.to_owned() });
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
        let waiting: Vec<Pending> = self.pending.drain().map(|(_, pending)| pending).collect();
        for pending in waiting {
            match pending {
                Pending::Evaluate(reply) => {
                    let _ = reply.send(Err("Chromium is gone".to_owned()));
                }
                Pending::Ask(tab) => {
                    self.emit(Event::Answered { tab, value: Err("Chromium is gone".to_owned()) });
                }
                Pending::World { reply, .. } | Pending::Reading(reply) => {
                    let _ = reply.send(Err("Chromium is gone".to_owned()));
                }
                Pending::Enabled(_) | Pending::Where(_) | Pending::Step(..) => {}
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

/// The function the selection watcher calls with the text the page has selected.
const SELECTION_BINDING: &str = "qbrowserSelection";

/// Reports the text the page has selected, once the gesture that made it has stilled.
///
/// A drag fires `selectionchange` for every cell it crosses, so reporting each one would send a
/// selection that is still being chosen again and again; the report waits for the quiet instead.
/// The empty string is reported like any other, so a selection that empties does not linger.
const SELECTION_WATCHER: &str = "(() => {
    let last = '';
    let quiet = null;
    const report = () => {
        const text = String(window.getSelection());
        if (text === last) {
            return;
        }
        last = text;
        clearTimeout(quiet);
        quiet = setTimeout(() => qbrowserSelection(last), 150);
    };
    document.addEventListener('selectionchange', report);
    report();
})();";

/// Has every document of the session report what it has selected, the way it reports its title.
fn watch_selection(wire: &mut Wire, session: &str) {
    // Bindings only report while the runtime's events are on.
    wire.call(Some(session), "Runtime.enable", json!({}));
    wire.call(Some(session), "Runtime.addBinding", json!({ "name": SELECTION_BINDING, "executionContextName": WORLD }));
    wire.call(
        Some(session),
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": SELECTION_WATCHER, "worldName": WORLD, "runImmediately": true }),
    );
}

/// The value of a `Runtime.evaluate` answer, or why there is none: the exception the expression
/// threw, its own text when it has none, or the call's error.
fn value_of(message: &Value) -> Result<Value, String> {
    let result = &message["result"];
    if let Some(error) = message.get("error") {
        return Err(error["message"].as_str().unwrap_or("the call failed").to_owned());
    }
    if let Some(exception) = result.get("exceptionDetails") {
        let text = exception["exception"]["description"].as_str().or_else(|| exception["text"].as_str());
        return Err(text.unwrap_or("the expression threw").to_owned());
    }
    Ok(result["result"].get("value").cloned().unwrap_or(Value::Null))
}
