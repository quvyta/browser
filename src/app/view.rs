//! Drawing the screen: the tab strip, the toolbar, and the page or what stands in its place.

use std::time::Duration;

use qframe::event::{Event, MouseButton, MouseKind};
use qframe::prelude::*;
use qframe::style::CellStyle;
use qframe::widget::{EventCx, MeasureCx, PaintCx, Widget};
use qframe::widgets::{Badge, EmptyState, IconButton, Spinner, TabWidth, Tabs, TextInput, Tooltip};

use super::{Browser, Missing, Msg, Phase};
use crate::page_view::PageView;

/// The rows above the page: the tab strip and the toolbar.
pub(super) const CHROME_ROWS: u16 = 2;

/// The name of the page area, which has the keyboard whenever nobody types an address.
pub(super) const PAGE: &str = "page";

/// The name of the address field, which takes the keyboard when it opens.
pub(super) const LOCATION: &str = "location";

/// Below this size nothing useful fits (design §2).
const SMALLEST: Size = Size { width: 30, height: 8 };

impl Browser {
    /// The whole screen at the size it is drawn in.
    pub(super) fn screen(&self, ui: &mut View<'_, Msg>) {
        let size = ui.size();
        if size.width < SMALLEST.width || size.height < SMALLEST.height {
            ui.add(EmptyState::new(t!("browser.too-small"))).fill();
            return;
        }
        match &self.phase {
            Phase::Missing(missing) => Self::missing(ui, missing),
            Phase::Failed(reason) => Self::failed(ui, t!("browser.chromium.failed"), reason),
            Phase::Gone(reason) => Self::failed(ui, t!("browser.chromium.stopped"), reason),
            Phase::Starting | Phase::Running => {
                AppShell::new()
                    .header(|ui| {
                        ui.column(|ui| {
                            self.strip(ui);
                            self.toolbar(ui);
                        })
                        .fill_width();
                    })
                    .body(|ui| self.page(ui))
                    .show(ui);
            }
        }
    }

    /// The tabs, each named by its page, with a close mark on each and `+` after them.
    fn strip(&self, ui: &mut View<'_, Msg>) {
        let labels = self.tabs.iter().map(|tab| tab.label().map_or_else(|| t!("browser.tab.new"), str::to_owned));
        ui.add(
            Tabs::new(labels)
                .active(self.active)
                .tab_width(TabWidth::Fill)
                .on_select(Msg::SelectTab)
                .closable(Msg::CloseTab)
                .on_add(|| Msg::NewTab),
        )
        .height(Length::Cells(1))
        .fill_width()
        .id("tabs");
    }

    /// Back, forward, reload or stop, the address, the loading mark and the temporary profile.
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
            ui.add(
                IconButton::new("chevron-right")
                    .tooltip(t!("browser.toolbar.forward"))
                    .disabled(!tab.can_forward)
                    .on_press(Msg::Forward),
            )
            .id("forward");
            if tab.loading {
                ui.add(IconButton::new("close").tooltip(t!("browser.toolbar.stop")).on_press(Msg::Stop)).id("stop");
            } else {
                ui.add(
                    IconButton::new("browser.reload")
                        .tooltip(t!("browser.toolbar.reload"))
                        .disabled(tab.id.is_none())
                        .on_press(Msg::Reload),
                )
                .id("reload");
            }
            match &self.location {
                Some(text) => {
                    ui.add(
                        TextInput::new(text.clone())
                            .placeholder(t!("browser.address.placeholder"))
                            .select_all_on_focus()
                            .on_change(Msg::LocationTyped)
                            .on_submit(Msg::Go),
                    )
                    .fill_width()
                    .id(LOCATION);
                }
                None => {
                    let address = AddressText {
                        address: tab.address().to_owned(),
                        placeholder: t!("browser.address.placeholder"),
                        on_press: Msg::Location(true),
                    };
                    ui.add(address).fill_width().id("address");
                }
            }
            let busy = self.phase == Phase::Starting || tab.busy();
            ui.add(Loading { busy }).id("loading");
            if self.temporary {
                ui.add_with(Tooltip::new(t!("browser.profile.temporary-explained")), |ui| {
                    ui.add(Badge::new(t!("browser.profile.temporary")).variant("warning"));
                })
                .id("temporary");
            }
        })
        .gap(1)
        .padding(Padding::symmetric(0, 1))
        .height(Length::Cells(1))
        .fill_width();
    }

    /// The page area: the page, or why it cannot be shown.
    fn page(&self, ui: &mut View<'_, Msg>) {
        let tab = self.tab();
        if tab.crashed {
            ui.add(
                EmptyState::new(t!("browser.tab.crashed"))
                    .icon("warning")
                    .message(t!("browser.tab.crashed-message"))
                    .action(Button::new(t!("browser.tab.reload")).variant("primary").on_press(Msg::Reload)),
            )
            .fill();
            return;
        }
        let drawn = self.graphics.can_draw();
        ui.stack(|ui| {
            ui.add(PageView::new(tab.picture.as_ref().filter(|_| drawn)).on_input(Msg::Page)).fill().id(PAGE);
            // Clicks still reach the page beneath: this says only why nothing is drawn. The page's
            // title and address stay on the strip and in the address bar.
            if !drawn {
                ui.add(EmptyState::new(t!("browser.page.cannot-show")).message(t!("browser.page.cannot-show-message")))
                    .fill();
            }
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
        ui.column(|ui| {
            ui.add(EmptyState::new(t!("browser.chromium.missing")).icon("warning").message(message)).fill_width();
            if let Some(command) = &missing.command {
                ui.row(|ui| {
                    ui.add(Button::new(t!("browser.chromium.install")).variant("primary").on_press(Msg::Install))
                        .id("install");
                    ui.add(Button::new(t!("browser.chromium.show-command")).on_press(Msg::ShowCommand))
                        .id("show-command");
                })
                .gap(2)
                .justify(Align::Center)
                .fill_width();
                if missing.command_shown {
                    ui.column(|ui| {
                        ui.add(Text::new(t!("browser.chromium.command")));
                        ui.add(Text::new(command.line())).selectable(true).id("command");
                    })
                    .align(Align::Center)
                    .padding(Padding::symmetric(1, 0))
                    .fill_width();
                }
            }
        })
        .gap(1)
        .justify(Align::Center)
        .fill();
    }

    /// Chromium did not start, or ended by itself: why, and the way to start it again.
    fn failed(ui: &mut View<'_, Msg>, title: String, reason: &str) {
        let mut state = EmptyState::new(title)
            .icon("error")
            .action(Button::new(t!("browser.chromium.restart")).variant("primary").on_press(Msg::Restart));
        if !reason.trim().is_empty() {
            state = state.message(t!("browser.chromium.reason", reason = reason));
        }
        ui.add(state).fill();
    }
}

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

/// How long loading runs before its mark shows: shorter work never blinks one (VISION §3).
const SHOW_AFTER: Duration = Duration::from_millis(300);

/// How long the mark stays once it shows, even when the loading ends sooner.
const SHOW_AT_LEAST: Duration = Duration::from_millis(500);

/// The loading mark: a spinner that shows only once loading has run 300 ms and then stays at
/// least 500 ms.
///
/// The framework keeps this rule inside its own widgets and offers no spinner that follows it,
/// so qbrowser keeps its own copy here until it does.
struct Loading {
    busy: bool,
}

/// Where the loading mark is in its life.
#[derive(Debug, Clone, Copy, Default)]
enum Mark {
    /// Nothing loads and nothing shows.
    #[default]
    Idle,
    /// Loading since this time; the mark waits for [`SHOW_AFTER`].
    Waiting(Duration),
    /// Shown since this time.
    Shown(Duration),
}

impl Mark {
    /// The mark at `now`, with loading `busy` or not.
    fn next(self, busy: bool, now: Duration) -> Self {
        let waiting = |since: Duration| if now >= since + SHOW_AFTER { Self::Shown(now) } else { Self::Waiting(since) };
        match (self, busy) {
            (Self::Idle | Self::Waiting(_), false) => Self::Idle,
            (Self::Idle, true) => waiting(now),
            (Self::Waiting(since), true) => waiting(since),
            (Self::Shown(since), true) => Self::Shown(since),
            (Self::Shown(since), false) if now < since + SHOW_AT_LEAST => Self::Shown(since),
            (Self::Shown(_), false) => Self::Idle,
        }
    }

    /// How long after `now` the mark may change while loading stays as it is.
    fn change_in(self, busy: bool, now: Duration) -> Option<Duration> {
        match self {
            Self::Waiting(since) => Some((since + SHOW_AFTER).saturating_sub(now)),
            Self::Shown(since) if !busy => Some((since + SHOW_AT_LEAST).saturating_sub(now)),
            Self::Idle | Self::Shown(_) => None,
        }
    }
}

impl Widget<Msg> for Loading {
    fn measure(&self, _cx: &mut MeasureCx<'_>, available: Size) -> Size {
        Size::new(1, 1).min(available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        let now = cx.now();
        let memory = cx.memory::<Mark>();
        *memory = memory.next(self.busy, now);
        let mark = *memory;
        if let Some(delay) = mark.change_in(self.busy, now) {
            cx.request_frame_in(delay);
        }
        if matches!(mark, Mark::Shown(_)) {
            Widget::<Msg>::paint(&Spinner::new(), cx, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn the_mark_waits_300_ms_and_then_stays_500_ms() {
        let quick = Mark::Idle.next(true, ms(0)).next(true, ms(299)).next(false, ms(299));
        assert!(matches!(quick, Mark::Idle), "quick loading never shows the mark");
        let shown = Mark::Idle.next(true, ms(0)).next(true, ms(300));
        assert!(matches!(shown, Mark::Shown(_)));
        assert!(matches!(shown.next(false, ms(799)), Mark::Shown(_)), "it stays half a second");
        assert!(matches!(shown.next(false, ms(800)), Mark::Idle));
        assert_eq!(Mark::Idle.next(true, ms(100)).change_in(true, ms(150)), Some(ms(250)));
    }
}
