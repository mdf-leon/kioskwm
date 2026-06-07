use std::{thread, time::Duration};

use smithay::{
    backend::input::{
        AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::pointer::{ButtonEvent, MotionEvent},
    utils::{Logical, Point},
};

use crate::{emergency::EmergencyContext, overlay::OverlayControl, state::State};

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

fn send_motion(
    state: &mut State,
    pointer: &smithay::input::pointer::PointerHandle<State>,
    location: Point<f64, Logical>,
    time: u32,
) {
    if state.overlay_open {
        return;
    }
    let serial = state.next_serial();
    let focus = state.pointer_focus();
    pointer.motion(
        state,
        focus,
        &MotionEvent {
            location,
            serial,
            time,
        },
    );
    pointer.frame(state);
}

/// Simula clique direito na posição atual do cursor (só para debug).
pub fn debug_right_click(state: &mut State, tracker: &PointerTracker) {
    tracing::info!(
        "DEBUG: simulando clique direito em ({:.0}, {:.0})",
        tracker.pos.x,
        tracker.pos.y
    );
    let pointer = state.pointer.clone();
    let time = 0u32;
    send_motion(state, &pointer, tracker.pos, time);

    let press_serial = state.next_serial();
    tracing::info!("DEBUG: botão direito press serial={press_serial:?}");
    pointer.button(
        state,
        &ButtonEvent {
            serial: press_serial,
            time,
            button: 273,
            state: ButtonState::Pressed,
        },
    );
    pointer.frame(state);

    thread::sleep(Duration::from_millis(150));

    let release_serial = state.next_serial();
    tracing::info!("DEBUG: botão direito release serial={release_serial:?}");
    pointer.button(
        state,
        &ButtonEvent {
            serial: release_serial,
            time,
            button: 273,
            state: ButtonState::Released,
        },
    );
    pointer.frame(state);
}

pub fn handle_input<B: InputBackend>(
    state: &mut State,
    tracker: &mut PointerTracker,
    overlay: &OverlayControl,
    emergency: &EmergencyContext,
    event: InputEvent<B>,
) {
    overlay.poll(state);

    let keyboard = state.keyboard.clone();
    let pointer = state.pointer.clone();
    let speed = state.pointer_speed;

    match event {
        InputEvent::Keyboard { event } => {
            let key_state = event.state();
            let serial = state.next_serial();
            keyboard.input::<(), _>(
                state,
                event.key_code(),
                key_state,
                serial,
                event.time() as u32,
                |data, mods, keysym| {
                    crate::emergency::compositor_keyboard_filter(
                        data, emergency, mods, keysym, key_state,
                    )
                },
            );
        }
        InputEvent::PointerMotion { event } => {
            let motion_scale = if state.overlay_open { 1.0 } else { speed };
            tracker.pos.x += event.delta_x() * motion_scale;
            tracker.pos.y += event.delta_y() * motion_scale;
            tracker.clamp(state.output_size);
            state.pointer_pos = tracker.pos;
            if state.overlay_open {
                crate::settings::input::handle_pointer_motion(state, tracker.pos);
            } else {
                send_motion(state, &pointer, tracker.pos, event.time() as u32);
            }
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let pos = Point::<f64, Logical>::from((event.x(), event.y()));
            tracker.pos = pos;
            tracker.clamp(state.output_size);
            state.pointer_pos = tracker.pos;
            if state.overlay_open {
                crate::settings::input::handle_pointer_motion(state, tracker.pos);
            } else {
                send_motion(state, &pointer, tracker.pos, event.time() as u32);
            }
        }
        InputEvent::PointerButton { event } => {
            if state.overlay_open {
                const BTN_LEFT: u32 = 0x110;
                if event.button_code() == BTN_LEFT {
                    let pressed = event.state() == ButtonState::Pressed;
                    crate::settings::input::handle_pointer_button(
                        state,
                        tracker.pos,
                        pressed,
                    );
                    if !pressed {
                        crate::settings::input::handle_pointer_release(state);
                    }
                }
                return;
            }
            let time = event.time() as u32;
            tracing::debug!(
                "botão {} {:?} em ({:.0}, {:.0})",
                event.button_code(),
                event.state(),
                state.pointer_pos.x,
                state.pointer_pos.y
            );
            send_motion(state, &pointer, state.pointer_pos, time);
            let serial = state.next_serial();
            pointer.button(
                state,
                &ButtonEvent {
                    serial,
                    time,
                    button: event.button_code(),
                    state: event.state(),
                },
            );
            pointer.frame(state);
        }
        InputEvent::PointerAxis { event } => {
            if state.overlay_open {
                return;
            }
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
