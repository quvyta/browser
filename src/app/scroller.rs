//! The framework's scroll view with Space added: a person reading scrolls with Space as readily
//! as with the arrows, and the framework's view knows the arrows, the page keys, Home and End.
//!
//! Only that one key is added here; everything else is the framework's own. Once the framework's
//! view pages down with Space itself this wrapper goes.

use qframe::event::{Event, KeyEvent};
use qframe::geometry::{Rect, Size};
use qframe::keymap::{Key, KeyChord};
use qframe::widget::{Container, EventCx, MeasureCx, Node, PaintCx, Widget};
use qframe::widgets::ScrollView;

/// The framework's scroll view with Space added, so that a person reading a long page scrolls
/// with Space as readily as with the arrows.
pub(super) struct Scroller<Msg> {
    view: ScrollView<Msg>,
}

impl<Msg: 'static> Scroller<Msg> {
    /// An empty scroll view; its content is added with
    /// [`View::add_with`](qframe::widget::View::add_with).
    pub(super) fn new() -> Self {
        Self { view: ScrollView::new() }
    }
}

impl<Msg: 'static> Container<Msg> for Scroller<Msg> {
    fn set_children(&mut self, children: Vec<Node<Msg>>) {
        self.view.set_children(children);
    }
}

impl<Msg: 'static> Widget<Msg> for Scroller<Msg> {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.view.measure(cx, available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        self.view.paint(cx, area);
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        if let Event::Key(key) = event
            && key.is_plain(Key::Space)
        {
            let down = Event::Key(KeyEvent::from_chord(KeyChord::plain(Key::PageDown)));
            return self.view.event(cx, &down);
        }
        self.view.event(cx, event)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn children(&self) -> &[Node<Msg>] {
        self.view.children()
    }

    fn children_mut(&mut self) -> &mut [Node<Msg>] {
        self.view.children_mut()
    }
}
