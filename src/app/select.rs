//! A page's drop-down list (`<select>`), shown by qbrow itself.
//!
//! Headless Chromium opens a select's list as a window of its own and never draws it into the
//! page's picture, so the person would choose blind. A left press on the page therefore first asks
//! the page, in the isolated world its own scripts cannot see, what is under the pointer; a
//! single-choice select is not handed the press, and its options open as a framework list just
//! under the element, as wide as it is. Every other press goes to the page as before.

use std::time::Duration;

use qframe::event::{Event, KeyKind};
use qframe::keymap::Key;
use qframe::prelude::*;
use qframe::runtime::Command;
use qframe::widget::{EventCx, MeasureCx, PaintCx, Widget};
use qframe::widgets::{List, ListItem, Popover};
use serde_json::Value;

use super::{Browser, Msg, engine_link, view};
use crate::engine::{KeyPress, TabId};

/// The name of the list of options, which takes the keyboard while it is open.
pub(super) const OPTIONS: &str = "select-options";

/// How long a press waits for the page to say what is under it. The page answers in a few
/// milliseconds; a page too busy to answer in this time gets its press as it would have anyway.
const ASK_WITHIN: Duration = Duration::from_millis(250);

/// How long choosing an option waits for the page to take it.
const CHOOSE_WITHIN: Duration = Duration::from_secs(1);

/// The most rows the list shows at once; a longer one scrolls.
const MOST_ROWS: u16 = 10;

/// One row of the list: an `optgroup`'s heading, or an option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Row {
    /// A group's heading, which cannot be chosen.
    Group(String),
    /// An option.
    Option {
        /// What the option says.
        label: String,
        /// Its place among the select's options, which is what `selectedIndex` is set to.
        index: usize,
        /// Whether the page lets it be chosen.
        disabled: bool,
    },
}

/// An open list of a page's select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DropDown {
    /// The tab whose page the select is in; another tab's list is not shown over this one.
    pub(super) tab: TabId,
    /// The select's own cells in the page area: the list opens just under them, as wide as they.
    pub(super) cells: Rect,
    /// The rows, in the select's own order.
    pub(super) rows: Vec<Row>,
    /// The option the select holds now.
    pub(super) selected: Option<usize>,
    /// The row the arrows are on.
    pub(super) highlight: Option<usize>,
}

impl DropDown {
    /// The list a page's answer describes, or `None` when the answer is not a select's.
    fn read(tab: TabId, answer: &Value, cell: (u32, u32)) -> Option<Self> {
        let rows: Vec<Row> = answer.get("rows")?.as_array()?.iter().filter_map(row).collect();
        let number = |key: &str| answer.get(key).and_then(Value::as_f64);
        let (left, top, right, bottom) = (number("left")?, number("top")?, number("right")?, number("bottom")?);
        let (width, height) = (f64::from(cell.0.max(1)), f64::from(cell.1.max(1)));
        let column = (left / width).floor();
        let line = (top / height).floor();
        let columns = ((right / width).ceil() - column).max(1.0);
        let lines = ((bottom / height).ceil() - line).max(1.0);
        #[expect(clippy::cast_possible_truncation, reason = "cells of a page area, far inside i32 and u16")]
        let cells = Rect::new(
            column as i32,
            line as i32,
            columns.min(f64::from(u16::MAX)) as u16,
            lines.min(f64::from(u16::MAX)) as u16,
        );
        let selected = answer.get("selected").and_then(Value::as_u64).and_then(|at| usize::try_from(at).ok());
        let highlight =
            rows.iter().position(|row| matches!(row, Row::Option { index, .. } if Some(*index) == selected));
        Some(Self { tab, cells, rows, selected, highlight })
    }

    /// The option on row `row`, when it is one that can be chosen.
    fn choosable(&self, row: usize) -> Option<usize> {
        match self.rows.get(row)? {
            Row::Option { index, disabled: false, .. } => Some(*index),
            _ => None,
        }
    }

    /// The next row after the highlight whose option can be chosen and starts with `letter`,
    /// going round the end, as a desktop browser's list jumps on a typed letter.
    fn jump(&self, letter: char) -> Option<usize> {
        let start = self.highlight.map_or(0, |at| at + 1);
        let wanted = letter.to_lowercase().collect::<String>();
        (0..self.rows.len()).map(|step| (start + step) % self.rows.len()).find(|at| {
            matches!(&self.rows[*at], Row::Option { label, disabled: false, .. }
                if label.trim_start().to_lowercase().starts_with(&wanted))
        })
    }
}

/// One row of the page's answer.
fn row(value: &Value) -> Option<Row> {
    if let Some(group) = value.get("group").and_then(Value::as_str) {
        return Some(Row::Group(group.to_owned()));
    }
    Some(Row::Option {
        label: value.get("label")?.as_str()?.to_owned(),
        index: usize::try_from(value.get("index")?.as_u64()?).ok()?,
        disabled: value.get("disabled").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// The question a press asks the page: the select under the page-area pixel (`x`, `y`), or the
/// one that has the keyboard when there is no point, described for the list; `null` for anything
/// else. The point is in the page area's own pixels, the ones the mouse is sent in, and the page's
/// own ratio, which is the zoom, turns it into its CSS pixels.
///
/// The select is kept in the isolated world's own global for the choice that follows; the page's
/// scripts live in another world and never see it. It is also given the keyboard, as a press on it
/// would have done, so the arrows work on it afterwards.
fn question(point: Option<(f64, f64)>) -> String {
    let at = point.map_or_else(
        || "document.activeElement".to_owned(),
        |(x, y)| format!("document.elementFromPoint({x} / ratio, {y} / ratio)"),
    );
    format!(
        "(() => {{
            const ratio = window.devicePixelRatio || 1;
            const at = {at};
            const select = at && at.closest ? at.closest('select') : null;
            if (!select || select.matches(':disabled') || select.multiple || select.size > 1) return null;
            window.qbrowserSelect = select;
            select.focus();
            const rows = [];
            const option = (o, off) => rows.push({{ label: o.label, index: o.index, disabled: o.disabled || off }});
            for (const child of select.children) {{
                if (child.tagName === 'OPTGROUP') {{
                    rows.push({{ group: child.label }});
                    for (const o of child.children) if (o.tagName === 'OPTION') option(o, child.disabled);
                }} else if (child.tagName === 'OPTION') option(child, false);
            }}
            const box = select.getBoundingClientRect();
            return {{ left: box.left * ratio, top: box.top * ratio, right: box.right * ratio,
                      bottom: box.bottom * ratio, selected: select.selectedIndex, rows }};
        }})()"
    )
}

/// What choosing option `index` does on the page: what a person's choice does, the value set and
/// `input` and `change` sent so the page's own listeners hear it; nothing is sent when the option
/// was already the one chosen, as in a desktop browser.
fn choice(index: usize) -> String {
    format!(
        "(() => {{
            const select = window.qbrowserSelect;
            if (!select || !select.isConnected) return false;
            if (select.selectedIndex === {index}) return true;
            select.selectedIndex = {index};
            select.dispatchEvent(new Event('input', {{ bubbles: true }}));
            select.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return true;
        }})()"
    )
}

/// Whether `press` is one that opens a select's list from the keyboard: Alt+↓, F4 or Space.
pub(super) fn opens_list(press: &KeyPress) -> bool {
    let mods = press.modifiers;
    let none = !mods.alt && !mods.ctrl && !mods.meta && !mods.shift;
    (press.key == "ArrowDown" && mods.alt && !mods.ctrl && !mods.meta)
        || (none && (press.key == "F4" || press.key == " "))
}

impl Browser {
    /// Opens the list of the select at the page-area cell (`column`, `row`), or of the one with
    /// the keyboard when there is no cell; `None` when there is no such select, and the press then
    /// goes to the page.
    pub(super) fn open_drop_down(&mut self, cell: Option<(u16, u16)>) -> Option<Command<Msg>> {
        let point = cell.map(|(column, row)| engine_link::cell_middle(column, row, self.cell));
        let (engine, tab) = (self.engine.as_ref()?, self.tab().id.clone()?);
        let answer = engine.evaluate_isolated(&tab, &question(point), ASK_WITHIN).ok()?;
        let list = DropDown::read(tab, &answer, self.cell)?;
        self.drop_down = Some(list);
        Some(Command::focus(OPTIONS))
    }

    /// The person chose row `row`: a heading or a disabled option keeps the list open and the
    /// page as it was; an option is set on the page and the list closes.
    pub(super) fn choose_option(&mut self, row: usize) -> Command<Msg> {
        let Some(list) = &self.drop_down else { return Command::none() };
        let Some(index) = list.choosable(row) else { return Command::none() };
        let tab = list.tab.clone();
        self.drop_down = None;
        if let Some(engine) = &self.engine {
            // An option the page no longer has is its own business; the list closes either way.
            let _ = engine.evaluate_isolated(&tab, &choice(index), CHOOSE_WITHIN);
        }
        Command::focus(view::PAGE)
    }

    /// The list closes with nothing chosen.
    pub(super) fn close_drop_down(&mut self) -> Command<Msg> {
        if self.drop_down.take().is_some() { Command::focus(view::PAGE) } else { Command::none() }
    }

    /// The arrows moved to row `row`.
    pub(super) fn highlight_option(&mut self, row: usize) {
        if let Some(list) = &mut self.drop_down {
            list.highlight = Some(row);
        }
    }

    /// A letter typed in the list: the next option that starts with it.
    pub(super) fn jump_to_option(&mut self, letter: char) {
        if let Some(list) = &mut self.drop_down
            && let Some(at) = list.jump(letter)
        {
            list.highlight = Some(at);
        }
    }

    /// The open list over the page area, when it belongs to the tab on screen: a layer just under
    /// the select's cells, as wide as they are.
    pub(super) fn drop_down_layer(&self, ui: &mut View<'_, Msg>) {
        let Some(list) = self.drop_down.as_ref().filter(|list| self.tab().id.as_ref() == Some(&list.tab)) else {
            return;
        };
        let check = ui.env().icons().glyph("check").into_owned();
        let items = list.rows.iter().map(|row| match row {
            Row::Group(label) => ListItem::header(label.clone()),
            Row::Option { label, index, disabled } => {
                let item = ListItem::new(label.clone()).faint(*disabled);
                // The option the select holds is marked by a sign, not by colour alone.
                if list.selected == Some(*index) { item.detail(check.clone()) } else { item }
            }
        });
        let options =
            List::new(items).selected(list.highlight).on_select(Msg::HighlightOption).on_activate(Msg::ChooseOption);
        let shown = u16::try_from(list.rows.len()).unwrap_or(MOST_ROWS).clamp(1, MOST_ROWS);
        ui.place(list.cells, |ui| {
            Popover::new(true)
                .match_anchor_width(true)
                .focus_inside(true)
                .on_dismiss(Msg::CloseOptions)
                .anchor(|_| {})
                .content(|ui| {
                    ui.add(Letters(options)).fill_width().height(Length::Cells(shown)).id(OPTIONS);
                })
                .show(ui)
                .fill();
        });
    }
}

/// The list of options, which also jumps on a typed letter. The framework's list moves on `j` and
/// `k`; a select's list is one where a letter means the option it starts, so letters are taken
/// here first.
struct Letters(List<Msg>);

impl Widget<Msg> for Letters {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.0.measure(cx, available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        self.0.paint(cx, area);
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        if let Event::Key(key) = event
            && key.kind != KeyKind::Release
            && !key.chord.mods.ctrl
            && !key.chord.mods.alt
            && let Key::Char(letter) = key.chord.key
            && letter.is_alphanumeric()
        {
            cx.emit(Msg::JumpToOption(letter));
            return true;
        }
        self.0.event(cx, event)
    }

    fn focusable(&self) -> bool {
        self.0.focusable()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// An answer as the page gives it for a select at 40,40 to 240,80 page pixels.
    fn answer() -> Value {
        json!({
            "left": 40.0, "top": 40.0, "right": 240.0, "bottom": 80.0, "selected": 1,
            "rows": [
                { "label": "Apple", "index": 0, "disabled": false },
                { "label": "Banana", "index": 1, "disabled": false },
                { "label": "Cherry", "index": 2, "disabled": true },
                { "group": "Citrus" },
                { "label": "Lemon", "index": 3, "disabled": false },
                { "label": "Lime", "index": 4, "disabled": false },
            ],
        })
    }

    #[test]
    fn the_list_stands_on_the_selects_own_cells_with_the_held_option_highlighted() {
        let list = DropDown::read(TabId("t".into()), &answer(), (10, 20)).expect("a select's answer");
        assert_eq!(list.cells, Rect::new(4, 2, 20, 2), "from its left and top edge to its right and bottom one");
        assert_eq!(list.highlight, Some(1), "the arrows start on the option the select holds");
        assert_eq!(list.selected, Some(1));
        assert_eq!(list.choosable(0), Some(0));
        assert_eq!(list.choosable(2), None, "a disabled option cannot be chosen");
        assert_eq!(list.choosable(3), None, "nor a group's heading");
        assert_eq!(list.choosable(4), Some(3), "a grouped option keeps the select's own index");
        assert!(DropDown::read(TabId("t".into()), &Value::Null, (10, 20)).is_none(), "no select, no list");
    }

    #[test]
    fn a_letter_jumps_to_the_next_option_it_starts_and_skips_what_cannot_be_chosen() {
        let mut list = DropDown::read(TabId("t".into()), &answer(), (10, 20)).unwrap();
        assert_eq!(list.jump('l'), Some(4), "Lemon, after the heading");
        list.highlight = Some(4);
        assert_eq!(list.jump('L'), Some(5), "then Lime, whatever the case");
        list.highlight = Some(5);
        assert_eq!(list.jump('l'), Some(4), "and round again");
        assert_eq!(list.jump('c'), None, "Cherry is disabled and Citrus is a heading");
    }

    #[test]
    fn alt_down_f4_and_space_open_the_list_and_plain_arrows_do_not() {
        let press = |key: &str, alt: bool| KeyPress {
            key: key.to_owned(),
            code: String::new(),
            key_code: 0,
            text: None,
            modifiers: crate::engine::Modifiers { alt, ..Default::default() },
        };
        assert!(opens_list(&press("ArrowDown", true)));
        assert!(opens_list(&press("F4", false)));
        assert!(opens_list(&press(" ", false)));
        assert!(!opens_list(&press("ArrowDown", false)), "a plain arrow changes the value on the page");
        assert!(!opens_list(&press("F4", true)));
        assert!(!opens_list(&press("a", false)));
    }
}
