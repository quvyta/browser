//! Drawing the screen: the tab strip, the toolbar, and the page or what stands in its place.

use qframe::event::{Event, MouseButton, MouseKind};
use qframe::prelude::*;
use qframe::style::CellStyle;
use qframe::widget::{EventCx, MeasureCx, PaintCx, Widget};
use qframe::widgets::{
    Badge, ContextMenu, EmptyState, HelpLayer, IconButton, Spinner, TabWidth, Tabs, ToastKind, Tooltip,
};

use super::menu::PageUnder;
use super::reader;
use super::{Browser, Missing, Msg, Phase, history::Arrow};
use crate::page_view::PageView;

/// The rows above the page: the tab strip and the toolbar. The bar of bookmarks adds one while
/// there are any.
pub(super) const CHROME_ROWS: u16 = 2;

/// The name of the page area, which has the keyboard whenever nobody types an address.
pub(super) const PAGE: &str = "page";

/// The name of the address field, which takes the keyboard when it opens.
pub(super) const LOCATION: &str = "location";

/// The name of the settings screen, which takes the keyboard while it is open.
pub(super) const SETTINGS: &str = "settings";

/// The name of the reading area, which holds the keyboard while the page is being read as text.
pub(super) const READING: &str = "reading";

/// The name of the settings screen's list of rows, the part of it the arrow keys walk.
pub(super) const SETTINGS_ROWS: &str = "settings-rows";

/// The name of the start page's field on that screen.
pub(super) const START_PAGE: &str = "start-page";

/// Below this size the tabs, the toolbar and a readable piece of the page no longer fit together.
const SMALLEST: Size = Size { width: 30, height: 8 };

impl Browser {
    /// The whole screen at the size it is drawn in.
    pub(super) fn screen(&self, ui: &mut View<'_, Msg>) {
        self.seen_cell.set(ui.env().cell_pixels());
        let size = ui.size();
        if size.width < SMALLEST.width || size.height < SMALLEST.height {
            ui.add(EmptyState::new(t!("browser.too-small"))).fill();
            return;
        }
        AppShell::new()
            .header(|ui| {
                ui.column(|ui| {
                    self.strip(ui);
                    self.toolbar(ui);
                    self.bookmarks_bar(ui);
                })
                .fill_width();
            })
            .body(|ui| self.body(ui))
            .show(ui);
        self.dialog(ui);
        if self.help_open {
            // The one rule the keymap cannot list: every key not on it goes to the page.
            ui.add(HelpLayer::new(Msg::Help(false)).hint("*", t!("browser.help.page")));
        }
    }

    /// The page area: the page, the settings screen in its place, or why there is no page. It is
    /// the only part of the shell that changes, so a person whose Chromium is missing, failed or
    /// gone can still reach the settings from the gear and change how qbrowser looks and whether
    /// it asks for its updates.
    fn body(&self, ui: &mut View<'_, Msg>) {
        if self.settings_open {
            return self.settings_page(ui);
        }
        match &self.phase {
            Phase::Missing(missing) => Self::missing(ui, missing),
            Phase::Failed(reason) => Self::failed(ui, t!("browser.chromium.failed"), reason),
            Phase::Gone(reason) => Self::failed(ui, t!("browser.chromium.stopped"), reason),
            Phase::Starting | Phase::Running if self.history_open => self.history_screen(ui),
            Phase::Starting | Phase::Running => self.page(ui),
        }
    }

    /// The tabs, each named by its page, with a close mark on each and `+` after them.
    fn strip(&self, ui: &mut View<'_, Msg>) {
        let labels = self.tabs.iter().map(|tab| tab.label().map_or_else(|| t!("browser.tab.new"), str::to_owned));
        ui.add(
            Tabs::new(labels)
                .active(self.active)
                .tab_width(TabWidth::Fill)
                .max_tab_width(TAB_WIDEST)
                .on_select(Msg::SelectTab)
                .closable(Msg::CloseTab)
                .on_add(|| Msg::NewTab),
        )
        .height(Length::Cells(1))
        .fill_width()
        .id("tabs");
    }

    /// Back, forward, reload or stop, the address, the star, the loading mark, the temporary
    /// profile and the settings gear.
    fn toolbar(&self, ui: &mut View<'_, Msg>) {
        let tab = self.tab();
        ui.row(|ui| {
            ui.add(
                IconButton::new("chevron-left")
                    .tooltip(t!("browser.toolbar.back"))
                    .disabled(!tab.can_back)
                    .on_press(Msg::Back),
            )
            .id("back");
            self.steps_button(ui, Arrow::Back);
            ui.add(
                IconButton::new("chevron-right")
                    .tooltip(t!("browser.toolbar.forward"))
                    .disabled(!tab.can_forward)
                    .on_press(Msg::Forward),
            )
            .id("forward");
            self.steps_button(ui, Arrow::Forward);
            if tab.loading {
                ui.add(IconButton::new("close").tooltip(t!("browser.toolbar.stop")).on_press(Msg::Stop)).id("stop");
            } else {
                ui.add(
                    IconButton::new("refresh")
                        .tooltip(t!("browser.toolbar.reload"))
                        .disabled(tab.id.is_none())
                        .on_press(Msg::Reload),
                )
                .id("reload");
            }
            match &self.location {
                Some(text) => self.address_field(ui, text),
                None => {
                    let address = AddressText {
                        address: tab.address().to_owned(),
                        placeholder: t!("browser.address.placeholder"),
                        on_press: Msg::Location(true),
                    };
                    ui.add(address).fill_width().id("address");
                }
            }
            self.star(ui);
            self.reader(ui);
            self.zoom(ui);
            let busy = self.phase == Phase::Starting || tab.busy();
            ui.add(Spinner::new().delayed(busy)).id("loading");
            if self.temporary {
                ui.add_with(Tooltip::new(t!("browser.profile.temporary-explained")), |ui| {
                    ui.add(Badge::new(t!("browser.profile.temporary")).variant("warning"));
                })
                .id("temporary");
            }
            ui.add(
                IconButton::new("settings")
                    .tooltip(t!("browser.settings.title"))
                    .on_press(Msg::Settings(!self.settings_open)),
            )
            .id(SETTINGS);
        })
        .gap(1)
        .padding(Padding::symmetric(0, 1))
        .height(Length::Cells(1))
        .fill_width();
    }

    /// The reading button, beside the star: the page's own text where the page's picture is, for a
    /// terminal that draws pictures too small to read and for one that draws none at all.
    fn reader(&self, ui: &mut View<'_, Msg>) {
        let open = self.reading();
        ui.add(
            IconButton::new("browser.reader")
                .tooltip(t!("browser.toolbar.reader"))
                .selected(open)
                .disabled(self.tab().id.is_none())
                .on_press(Msg::Reader(!open)),
        )
        .id("reader");
    }

    /// The page area: the page, reading mode in its place, or why the page cannot be shown.
    fn page(&self, ui: &mut View<'_, Msg>) {
        let tab = self.tab();
        if tab.crashed {
            ui.add(
                EmptyState::new(t!("browser.tab.crashed"))
                    .tone(ToastKind::Danger)
                    .message(t!("browser.tab.crashed-message"))
                    .action(Button::new(t!("browser.tab.reload")).variant("primary").on_press(Msg::Reload)),
            )
            .fill();
            return;
        }
        if !matches!(tab.reading, reader::Mode::Closed) {
            return self.reading_page(ui, &tab.reading);
        }
        let drawn = self.graphics.can_draw();
        ui.add_with(ContextMenu::new(self.menu_items()), |ui| {
            ui.stack(|ui| {
                ui.add_with(PageUnder::new(|(column, row)| Msg::PageRight { column, row }), |ui| {
                    ui.add(PageView::new(tab.picture.as_ref().filter(|_| drawn)).on_input(Msg::Page)).fill();
                })
                .fill()
                .id(PAGE);
                // Clicks still reach the page beneath: this says only why nothing is drawn. The page's
                // title and address stay on the strip and in the address bar, and the page is still
                // there to be read as text.
                if !drawn {
                    ui.add(
                        EmptyState::new(t!("browser.page.cannot-show"))
                            .message(t!("browser.page.cannot-show-message"))
                            .action(
                                Button::new(t!("browser.reader.open")).variant("primary").on_press(Msg::Reader(true)),
                            ),
                    )
                    .fill();
                }
                self.drop_down_layer(ui);
            });
        })
        .fill();
    }

    /// Chromium is not on this machine: what it is for, and the way to install it here.
    fn missing(ui: &mut View<'_, Msg>, missing: &Missing) {
        let mut message = t!("browser.chromium.missing-message");
        if missing.command.is_none() {
            message = format!("{message} {}", t!("browser.chromium.unknown-system"));
        }
        if missing.still_missing {
            message = format!("{message} {}", t!("browser.chromium.still-missing"));
        }
        let mut state = EmptyState::new(t!("browser.chromium.missing")).tone(ToastKind::Warning).message(message);
        if missing.command.is_some() {
            state = state
                .action(Button::new(t!("browser.chromium.install")).variant("primary").on_press(Msg::Install))
                .action(Button::new(t!("browser.chromium.show-command")).on_press(Msg::ShowCommand));
        }
        ui.column(|ui| {
            ui.add(state).fill_width();
            if let Some(command) = missing.command.as_ref().filter(|_| missing.command_shown) {
                ui.column(|ui| {
                    ui.add(Text::new(t!("browser.chromium.command")));
                    ui.add(Text::new(command.line())).selectable(true).id("command");
                })
                .align(Align::Center)
                .padding(Padding::symmetric(1, 0))
                .fill_width();
            }
        })
        .gap(1)
        .justify(Align::Center)
        .fill();
    }

    /// Chromium did not start, or ended by itself: why, and the way to start it again.
    fn failed(ui: &mut View<'_, Msg>, title: String, reason: &str) {
        let mut state = EmptyState::new(title)
            .tone(ToastKind::Danger)
            .action(Button::new(t!("browser.chromium.restart")).variant("primary").on_press(Msg::Restart));
        if !reason.trim().is_empty() {
            state = state.message(t!("browser.chromium.reason", reason = reason));
        }
        ui.add(state).fill();
    }
}

/// The widest a tab grows, in cells: about as wide as a desktop browser's tab, so one tab does
/// not stretch across the whole strip with its close mark at the far edge. Below that the tabs
/// share the strip, down to the framework's readable floor, past which the strip scrolls.
const TAB_WIDEST: u16 = 28;

/// The address as plain text in the toolbar: a click on it turns it into the field.
struct AddressText {
    address: String,
    placeholder: String,
    on_press: Msg,
}

impl Widget<Msg> for AddressText {
    fn measure(&self, _cx: &mut MeasureCx<'_>, available: Size) -> Size {
        Size::new(available.width, 1).min(available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        // It stands on the field's own ground and brightens under the pointer as the field does,
        // so it reads as the place to type an address before it becomes one.
        let states = cx.pressable_states();
        let style = cx.style("text-input", None, &states).text();
        if let Some(bg) = style.bg {
            cx.fill(area, bg);
        }
        cx.register_hit(area);
        let (text, look) = if self.address.is_empty() {
            let placeholder = cx.style("text-input-placeholder", None, &states).text();
            (self.placeholder.as_str(), CellStyle { bg: None, ..placeholder })
        } else {
            (self.address.as_str(), CellStyle { bg: None, ..style })
        };
        cx.text(area.x + 1, area.y, text, look, area.width.saturating_sub(2));
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                cx.emit(self.on_press.clone());
                true
            }
            _ => false,
        }
    }
}
