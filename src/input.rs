use smithay::{
    backend::input::{
        AbsolutePositionEvent, Event, InputBackend, InputEvent, KeyboardKeyEvent, PointerAxisEvent,
        PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point},
};

use crate::state::State;

pub struct PointerTracker {
    pub pos: Point<f64, Logical>,
}

impl PointerTracker {
    pub fn new(size: smithay::utils::Size<i32, Logical>) -> Self {
        Self {
            pos: Point::from((size.w as f64 / 2.0, size.h as f64 / 2.0)),
        }
    }

    pub fn clamp(&mut self, size: smithay::utils::Size<i32, Logical>) {
        let max_x = (size.w.saturating_sub(1)).max(0) as f64;
        let max_y = (size.h.saturating_sub(1)).max(0) as f64;
        self.pos.x = self.pos.x.clamp(0.0, max_x);
        self.pos.y = self.pos.y.clamp(0.0, max_y);
    }
}

pub fn handle_input<B: InputBackend>(
    state: &mut State,
    tracker: &mut PointerTracker,
    event: InputEvent<B>,
) {
    let keyboard = state.keyboard.clone();
    let pointer = state.pointer.clone();

    match event {
        InputEvent::Keyboard { event } => {
            let serial = state.next_serial();
            keyboard.input::<(), _>(
                state,
                event.key_code(),
                event.state(),
                serial,
                event.time() as u32,
                |_, _, _| FilterResult::Forward,
            );
        }
        InputEvent::PointerMotion { event } => {
            tracker.pos.x += event.delta_x();
            tracker.pos.y += event.delta_y();
            tracker.clamp(state.output_size);

            let location = tracker.pos;
            let serial = state.next_serial();
            let focus = state.pointer_focus();
            pointer.motion(
                state,
                focus,
                &MotionEvent {
                    location,
                    serial,
                    time: event.time() as u32,
                },
            );
        }
        InputEvent::PointerMotionAbsolute { event } => {
            tracker.pos = Point::<f64, Logical>::from((event.x(), event.y()));
            tracker.clamp(state.output_size);

            let location = tracker.pos;
            let serial = state.next_serial();
            let focus = state.pointer_focus();
            pointer.motion(
                state,
                focus,
                &MotionEvent {
                    location,
                    serial,
                    time: event.time() as u32,
                },
            );
        }
        InputEvent::PointerButton { event } => {
            let serial = state.next_serial();
            pointer.button(
                state,
                &ButtonEvent {
                    serial,
                    time: event.time() as u32,
                    button: event.button_code(),
                    state: event.state(),
                },
            );
        }
        InputEvent::PointerAxis { event } => {
            use smithay::backend::input::{Axis, AxisRelativeDirection};
            use smithay::input::pointer::AxisFrame;

            let h = event.amount(Axis::Horizontal).unwrap_or(0.0);
            let v = event.amount(Axis::Vertical).unwrap_or(0.0);
            let v120_h = event.amount_v120(Axis::Horizontal).map(|a| a as i32);
            let v120_v = event.amount_v120(Axis::Vertical).map(|a| a as i32);

            pointer.axis(
                state,
                AxisFrame {
                    source: Some(event.source()),
                    relative_direction: (
                        AxisRelativeDirection::Identical,
                        AxisRelativeDirection::Identical,
                    ),
                    time: event.time() as u32,
                    axis: (h, v),
                    v120: match (v120_h, v120_v) {
                        (Some(x), Some(y)) => Some((x, y)),
                        _ => None,
                    },
                    stop: (false, false),
                },
            );
            pointer.frame(state);
        }
        _ => {}
    }
}
