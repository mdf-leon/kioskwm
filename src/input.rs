use std::{process::Command, thread, time::Duration};

use smithay::{
    backend::input::{
        AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, KeysymHandle, ModifiersState},
        pointer::{ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point},
};
use xkbcommon::xkb::keysyms;

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

fn vt_from_keysym(keysym: KeysymHandle<'_>) -> Option<u8> {
    match keysym.modified_sym().raw() {
        k if k == keysyms::KEY_F1 as u32 => Some(1),
        k if k == keysyms::KEY_F2 as u32 => Some(2),
        k if k == keysyms::KEY_F3 as u32 => Some(3),
        k if k == keysyms::KEY_F4 as u32 => Some(4),
        k if k == keysyms::KEY_F5 as u32 => Some(5),
        k if k == keysyms::KEY_F6 as u32 => Some(6),
        k if k == keysyms::KEY_F7 as u32 => Some(7),
        k if k == keysyms::KEY_F8 as u32 => Some(8),
        k if k == keysyms::KEY_F9 as u32 => Some(9),
        k if k == keysyms::KEY_F10 as u32 => Some(10),
        k if k == keysyms::KEY_F11 as u32 => Some(11),
        k if k == keysyms::KEY_F12 as u32 => Some(12),
        _ => None,
    }
}

fn request_vt_switch(vt: u8) {
    tracing::info!("Trocando para tty{vt}");
    std::thread::spawn(move || {
        let _ = Command::new("chvt").arg(vt.to_string()).status();
    });
}

fn keyboard_filter(
    modifiers: &ModifiersState,
    keysym: KeysymHandle<'_>,
    key_state: KeyState,
) -> FilterResult<()> {
    if key_state == KeyState::Pressed && modifiers.ctrl && modifiers.alt {
        if let Some(vt) = vt_from_keysym(keysym) {
            request_vt_switch(vt);
            return FilterResult::Intercept(());
        }
    }
    FilterResult::Forward
}

fn send_motion(
    state: &mut State,
    pointer: &smithay::input::pointer::PointerHandle<State>,
    location: Point<f64, Logical>,
    time: u32,
) {
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

    // Konsole abre o menu no release — espera o popup ser criado antes de soltar.
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
    event: InputEvent<B>,
) {
    let keyboard = state.keyboard.clone();
    let pointer = state.pointer.clone();

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
                |_, modifiers, keysym| keyboard_filter(modifiers, keysym, key_state),
            );
        }
        InputEvent::PointerMotion { event } => {
            tracker.pos.x += event.delta_x();
            tracker.pos.y += event.delta_y();
            tracker.clamp(state.output_size);
            state.pointer_pos = tracker.pos;
            send_motion(state, &pointer, tracker.pos, event.time() as u32);
        }
        InputEvent::PointerMotionAbsolute { event } => {
            tracker.pos = Point::<f64, Logical>::from((event.x(), event.y()));
            tracker.clamp(state.output_size);
            state.pointer_pos = tracker.pos;
            send_motion(state, &pointer, tracker.pos, event.time() as u32);
        }
        InputEvent::PointerButton { event } => {
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
