//! What a right click on the page offers, and what copying the page's selection needs: the menu's
//! entries, the question the page is asked about the element under the pointer, and the two
//! copies, clean and raw.

use qframe::event::{Event, KeyEvent, MouseButton, MouseEvent, MouseKind};
use qframe::geometry::{Rect, Size};
use qframe::keymap::{Key, Modifiers};
use qframe::prelude::*;
use qframe::runtime::Command;
use qframe::widget::{Container, EventCx, MeasureCx, Node, PaintCx, Widget};
use qframe::widgets::ContextItem;

use super::{Browser, Msg, engine_link};
use crate::keys;

impl Browser {
    /// The menu over the page: the selection's two copies, the way back and forward, the reload
    /// or the stop, and — when the page has said there is one — the link under the pointer.
    pub(super) fn menu_items(&self) -> Vec<ContextItem<Msg>> {
        let tab = self.tab();
        let nothing = tab.selection.is_empty();
        let mut items = Vec::new();
        if let Some(href) = self.link_under_pointer() {
            let href = href.to_owned();
            items.push(
                ContextItem::new(t!("browser.bookmarks.open-new-tab"), Msg::OpenLink(href.clone()))
                    .detail(href.clone()),
            );
            items.push(ContextItem::new(t!("browser.menu.copy-address"), Msg::CopyAddress(href.clone())).detail(href));
            items.push(ContextItem::gap());
        }
        items.push(ContextItem::new(t!("quvyta.edit.copy"), Msg::Copy).shortcut("ctrl+c").disabled(nothing));
        items.push(ContextItem::new(t!("quvyta.edit.raw-copy"), Msg::CopyRaw).disabled(nothing));
        items.push(ContextItem::gap());
        items
            .push(ContextItem::new(t!("browser.toolbar.back"), Msg::Back).shortcut("alt+left").disabled(!tab.can_back));
        items.push(
            ContextItem::new(t!("browser.toolbar.forward"), Msg::Forward)
                .shortcut("alt+right")
                .disabled(!tab.can_forward),
        );
        items.push(if tab.loading {
            ContextItem::new(t!("browser.toolbar.stop"), Msg::Stop)
        } else {
            ContextItem::new(t!("browser.toolbar.reload"), Msg::Reload).disabled(tab.id.is_none())
        });
        items
    }

    /// The address of the link under the pointer, when the last answer was about the tab on
    /// screen: an answer about another tab says nothing about this one.
    pub(super) fn link_under_pointer(&self) -> Option<&str> {
        let (tab, href) = self.link.as_ref()?;
        (self.tab().id.as_ref() == Some(tab)).then_some(href.as_str())
    }

    /// A right press on the page: the page is asked what is under the pointer. The answer comes
    /// on its own as [`crate::engine::Event::Answered`] and the link entries are added then; where
    /// there is no link there is nothing to add and nothing is shown in its place.
    pub(super) fn ask_about_link(&mut self, column: u16, row: u16) {
        // The last answer was about another press, and the menu is about to open without knowing
        // whether this one is over a link at all: its address must not be shown until it is known.
        self.link = None;
        let (x, y) = engine_link::cell_middle(column, row, self.cell);
        // The point is in the page area's own pixels, the ones the mouse is sent in; the page
        // turns them into its CSS pixels with its own ratio, which is the zoom, so the question
        // lands on what is drawn under the pointer at every zoom.
        self.on_page(|engine, id| {
            engine.ask(
                id,
                &format!(
                    "(() => {{ const r = window.devicePixelRatio || 1; const at = document.elementFromPoint({x} / r, {y} / r); const link = at && at.closest('a[href]'); return link ? link.href : null; }})()"
                ),
            );
        });
    }

    /// The copy key: the page's selection as it would be pasted into a field.
    pub(super) fn copy_selection(&mut self) -> Command<Msg> {
        let text = self.tab().selection.clone();
        if !text.is_empty() {
            return Command::copy(clean(&text));
        }
        // Nothing is selected, so the chord belongs to the page again: a page's own copy button
        // in a code block listens for it, and taking it would break a thing that works. Only
        // while the page has the keyboard, which it does not while the address bar is a field.
        let chord = keys::key_press(&KeyEvent::press("ctrl+c"));
        if self.location.is_none() {
            self.on_page(|engine, id| {
                if let Some(press) = &chord {
                    engine.key(id, press);
                }
            });
        }
        Command::none()
    }

    /// The raw copy: exactly the characters the page reported, so a snippet keeps its own shape.
    pub(super) fn copy_selection_raw(&mut self) -> Command<Msg> {
        let text = self.tab().selection.clone();
        if text.is_empty() {
            return Command::none();
        }
        Command::copy(text)
    }
}

/// The selection as it would be pasted into a field: runs of whitespace, newlines and tabs alike,
/// become one space and the two ends are trimmed.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Shift+F10 or the menu key, the framework's own rule for opening a menu from the keyboard.
fn is_menu_key(key: &KeyEvent) -> bool {
    let shift = Modifiers { shift: true, ..Modifiers::default() };
    (key.chord.key == Key::F(10) && key.chord.mods == shift) || key.is_plain(Key::Menu)
}

/// The page area inside a `ContextMenu`: it takes the right press and the menu keys for the menu
/// around it and hands everything else — the wheel, every other key, the left button — to the
/// page. It also holds the area's focus, since the keys belong to the menu before they reach the
/// page. It wraps the page area alone; a second widget in it would not be laid out or painted.
///
/// The framework's menu leaves the right click to plain content and takes it from an interactive
/// child instead, while a page takes every press. So a menu cannot open over a page without this,
/// and a browser needs the framework's menu to take a press from the widget under it.
pub(super) struct PageUnder<Msg> {
    body: Vec<Node<Msg>>,
    on_right: Box<dyn Fn((u16, u16)) -> Msg>,
}

impl<Msg> PageUnder<Msg> {
    /// The page area, which sends `on_right` the cell of every right press.
    pub(super) fn new(on_right: impl Fn((u16, u16)) -> Msg + 'static) -> Self {
        Self { body: Vec::new(), on_right: Box::new(on_right) }
    }
}

impl<Msg: 'static> Container<Msg> for PageUnder<Msg> {
    fn set_children(&mut self, children: Vec<Node<Msg>>) {
        self.body = children;
    }
}

impl<Msg: 'static> Widget<Msg> for PageUnder<Msg> {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.body.first().map_or(Size::default(), |body| cx.measure_child(body, available))
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        if let Some(body) = self.body.first() {
            cx.paint_child(body, area);
        }
        // Registered after the page, so a press lands here and the menu around this area is asked
        // before the page is.
        cx.register_hit(area);
        // The page takes text, and its keys come through here, so this area takes text as well.
        cx.takes_text();
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        let Some(body) = self.body.first() else { return false };
        let for_the_menu = match event {
            Event::Mouse(mouse) => {
                matches!(mouse.kind, MouseKind::Down(MouseButton::Right) | MouseKind::Up(MouseButton::Right))
            }
            Event::Key(key) => is_menu_key(key),
            _ => false,
        };
        if for_the_menu {
            if let Event::Mouse(MouseEvent { kind: MouseKind::Down(MouseButton::Right), x, y, .. }) = event {
                let area = cx.area();
                let cell = |at: i32, from: i32| u16::try_from((at - from).max(0)).unwrap_or_default();
                cx.emit((self.on_right)((cell(*x, area.x), cell(*y, area.y))));
            }
            // The menu around this area takes the press and the keys, so the page hears neither.
            return false;
        }
        cx.forward(body, cx.area(), event)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn children(&self) -> &[Node<Msg>] {
        &self.body
    }

    fn children_mut(&mut self) -> &mut [Node<Msg>] {
        &mut self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_copy_collapses_whitespace_and_trims_the_ends() {
        assert_eq!(clean("  hello   world  "), "hello world");
        assert_eq!(clean("first\n\t second \n\n third"), "first second third");
        assert_eq!(clean("one"), "one");
        assert_eq!(clean("   "), "");
        assert_eq!(clean(""), "");
    }

    #[test]
    fn a_clean_copy_leaves_a_snippet_on_one_line_where_a_raw_copy_keeps_its_shape() {
        let raw = "  let x = 1;\n    let y = 2;\n";
        assert_eq!(clean(raw), "let x = 1; let y = 2;");
        assert!(raw.contains('\n') && raw.contains("    "), "the raw text keeps its own lines and indent");
    }

    #[test]
    fn the_menu_keys_are_shift_f10_and_the_menu_key_alone() {
        assert!(is_menu_key(&KeyEvent::press("shift+f10")));
        assert!(is_menu_key(&KeyEvent::press("menu")));
        assert!(!is_menu_key(&KeyEvent::press("ctrl+shift+f10")));
        assert!(!is_menu_key(&KeyEvent::press("f10")));
        assert!(!is_menu_key(&KeyEvent::press("shift+f11")));
        assert!(!is_menu_key(&KeyEvent::press("a")));
    }
}
