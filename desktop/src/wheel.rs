//! Gives every scrollable browser-like wheel behaviour, once, at the root: one
//! notch travels about 100 px and glides there over a few frames instead of
//! jumping in a single frame, which is what Iced does on its own.
use iced::advanced::widget::{self, tree, Tree};
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::mouse::{self, ScrollDelta};
use std::cell::Cell;
use std::time::Instant;
use iced::{window, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

const PIXELS_PER_LINE: f32 = 100.0;
/// Smaller is snappier. Most of a notch lands within about 150 ms.
const GLIDE_SECONDS: f32 = 0.06;

/// Distance still to travel. There is one window, and only the UI thread touches this.
#[derive(Clone, Copy, Default)]
struct Glide { remaining: f32, carry: f32, last: Option<Instant> }
thread_local! {
    static GLIDE: Cell<Glide> = Cell::new(Glide::default());
    #[cfg(debug_assertions)]
    static TEST_CURSOR: Cell<bool> = const { Cell::new(false) };
}

fn add_lines(lines: f32) {
    let mut glide = GLIDE.get();
    let distance = lines * PIXELS_PER_LINE;
    // Reversing direction should respond at once, not finish the old glide first.
    glide.remaining = if glide.remaining * distance < 0.0 { distance } else { glide.remaining + distance };
    if glide.last.is_none() { glide.last = Some(Instant::now()); }
    GLIDE.set(glide);
}

/// Whole pixels to move this frame, so text never lands on a blurry half pixel.
fn advance(glide: &mut Glide, elapsed: f32) -> f32 {
    let mut step = glide.remaining * (1.0 - (-elapsed / GLIDE_SECONDS).exp());
    if step.abs() < 1.0 { step = glide.remaining.clamp(-1.0, 1.0); }
    glide.remaining -= step;
    let exact = step + glide.carry;
    let whole = exact.round();
    glide.carry = exact - whole;
    if glide.remaining.abs() < 0.5 { *glide = Glide::default(); }
    whole
}

/// Drives the glide without a real mouse, for scripted checks of development builds.
#[cfg(debug_assertions)]
pub fn test_notches(lines: f32) { TEST_CURSOR.set(true); add_lines(lines); }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_notch_glides_the_full_distance_in_whole_pixels() {
        let mut glide = Glide { remaining: 100.0, ..Default::default() };
        let (mut moved, mut frames, mut largest) = (0.0, 0, 0f32);
        while glide.remaining != 0.0 && frames < 500 {
            let step = advance(&mut glide, 1.0 / 60.0);
            assert_eq!(step, step.round());
            moved += step; frames += 1; largest = largest.max(step);
        }
        assert!((moved - 100.0_f32).abs() <= 1.0, "moved {moved}");
        assert!(frames > 5, "spread over {frames} frames");
        assert!(largest < 40.0, "largest step {largest}");
    }
    #[test]
    fn a_very_slow_frame_still_finishes_the_notch() {
        let mut glide = Glide { remaining: -100.0, ..Default::default() };
        assert!(advance(&mut glide, 0.05).abs() > 50.0);
    }
}

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
        if let Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Lines { x: 0.0, y } }) = event {
            add_lines(*y);
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let mut glide = GLIDE.get();
            if glide.remaining != 0.0 {
                // Capped so a stalled frame eases on rather than leaping.
                let elapsed = glide.last.map_or(0.016, |last| now.saturating_duration_since(last).as_secs_f32()).min(0.05);
                glide.last = Some(*now);
                let y = advance(&mut glide, elapsed);
                let more = glide.remaining != 0.0;
                GLIDE.set(glide);
                #[cfg(debug_assertions)]
                let cursor = if TEST_CURSOR.get() { mouse::Cursor::Available(layout.bounds().center() + Vector::new(300.0, 0.0)) } else { cursor };
                if y != 0.0 {
                    let scroll = Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Pixels { x: 0.0, y } });
                    self.content.as_widget_mut().update(tree, &scroll, layout, cursor, renderer, clipboard, shell, viewport);
                }
                if more { shell.request_redraw(); }
                #[cfg(debug_assertions)]
                if std::env::var_os("SONAR_DEV_WHEEL_LOG").is_some() { println!("WHEEL step {y} left {:.1}", GLIDE.get().remaining); }
            }
        }
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}
