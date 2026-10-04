//! A page's own dialogs: `alert`, `confirm`, `prompt` and the question before leaving a page.
//!
//! Chromium draws none of them: it stops the page and waits for an answer. Without one the page
//! would freeze with nothing on screen to say why, so each dialog is shown as a qframe dialog
//! over the tab it belongs to and the person's answer goes back to Chromium.

use qframe::prelude::*;
use qframe::widgets::{Modal, TextInput};

use super::{Browser, Msg};
use crate::engine::{DialogKind, Engine};

/// The dialog a tab's page waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PageDialog {
    pub(super) kind: DialogKind,
    pub(super) message: String,
    /// A prompt's answer as typed so far.
    pub(super) text: String,
}

/// The name of a prompt's text field, which has the keyboard while the dialog is open.
const ANSWER: &str = "dialog-answer";

impl Browser {
    /// The page of the tab on screen answered: `accept` is OK or "Leave", the prompt's text goes
    /// with it. The dialog closes.
    pub(super) fn answer_dialog(&mut self, accept: bool) {
        let Some(dialog) = self.tabs[self.active].dialog.take() else { return };
        let text = (dialog.kind == DialogKind::Prompt && accept).then_some(dialog.text);
        self.on_page(|engine: &Engine, id| engine.answer_dialog(id, accept, text.as_deref()));
    }

    /// The prompt's text changed.
    pub(super) fn dialog_typed(&mut self, text: String) {
        if let Some(dialog) = &mut self.tabs[self.active].dialog {
            dialog.text = text;
        }
    }

    /// The dialog of the tab on screen, over everything, while its page waits on one.
    pub(super) fn dialog(&self, ui: &mut View<'_, Msg>) {
        let Some(dialog) = &self.tab().dialog else { return };
        let host = host_of(self.tab().address());
        let (title, ok, cancel) = match dialog.kind {
            DialogKind::Alert => (t!("browser.dialog.says", host = host), t!("browser.dialog.ok"), None),
            DialogKind::Confirm | DialogKind::Prompt => {
                (t!("browser.dialog.asks", host = host), t!("browser.dialog.ok"), Some(t!("browser.dialog.cancel")))
            }
            DialogKind::BeforeUnload => {
                (t!("browser.dialog.leave-title"), t!("browser.dialog.leave"), Some(t!("browser.dialog.stay")))
            }
        };
        let mut modal = Modal::new().title(title).on_close(Msg::DialogAnswer(false));
        if let Some(cancel) = cancel {
            modal = modal.action(Button::new(cancel).on_press(Msg::DialogAnswer(false)));
        }
        modal = modal.action(Button::new(ok).variant("primary").on_press(Msg::DialogAnswer(true)));
        let message = match dialog.kind {
            DialogKind::BeforeUnload => t!("browser.dialog.leave-message"),
            _ => dialog.message.clone(),
        };
        ui.add_with(modal, |ui| {
            if !message.is_empty() {
                ui.add(Text::new(message)).id("dialog-message");
            }
            if dialog.kind == DialogKind::Prompt {
                ui.add(
                    TextInput::new(dialog.text.clone())
                        .on_change(Msg::DialogTyped)
                        .on_submit(|_| Msg::DialogAnswer(true)),
                )
                .id(ANSWER)
                .fill_width();
            }
        });
    }
}

/// The site a page is on, as the dialog's title names it: the host of an address, or the whole
/// address when it has none (a `file:` or `data:` page).
fn host_of(address: &str) -> String {
    let Some((_, rest)) = address.split_once("://") else { return address.to_owned() };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    if host.is_empty() { address.to_owned() } else { host.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::host_of;

    #[test]
    fn the_title_names_the_site_and_not_the_whole_address() {
        assert_eq!(host_of("http://127.0.0.1:8080/a/b?c#d"), "127.0.0.1:8080");
        assert_eq!(host_of("https://user@example.com/x"), "example.com");
        assert_eq!(host_of("data:text/html,hi"), "data:text/html,hi");
    }
}
