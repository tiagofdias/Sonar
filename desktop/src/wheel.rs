//! Makes one mouse-wheel notch travel as far as it does in a browser. Iced
//! moves 60 px per line with no setting to change it; this wrapper scales the
//! event once, at the root, so every scrollable in the app behaves the same.
use iced::advanced::widget::{self, tree, Tree};
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::mouse::{self, ScrollDelta};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

/// About 100 px per notch.
const LINE_SCALE: f32 = 1.67;

struct Wheel<'a, M> { content: Element<'a, M> }

pub fn browser_speed<'a, M: 'a>(content: Element<'a, M>) -> Element<'a, M> { Element::new(Wheel { content }) }

impl<M> Widget<M, Theme, Renderer> for Wheel<'_, M> {
    fn tag(&self) -> tree::Tag { self.content.as_widget().tag() }
    fn state(&self) -> tree::State { self.content.as_widget().state() }
    fn children(&self) -> Vec<Tree> { self.content.as_widget().children() }
    fn diff(&self, tree: &mut Tree) { self.content.as_widget().diff(tree); }
    fn size(&self) -> Size<Length> { self.content.as_widget().size() }
    fn size_hint(&self) -> Size<Length> { self.content.as_widget().size_hint() }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }
    fn update(&mut self, tree: &mut Tree, event: &Event, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer, clipboard: &mut dyn Clipboard, shell: &mut Shell<'_, M>, viewport: &Rectangle) {
        let scaled = match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Lines { x, y } }) =>
                Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Lines { x: x * LINE_SCALE, y: y * LINE_SCALE } }),
            other => other.clone(),
        };
        self.content.as_widget_mut().update(tree, &scaled, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}
