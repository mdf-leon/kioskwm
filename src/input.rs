use std::{thread, time::Duration};

use smithay::{
    backend::input::{
        AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    utils::{Logical, Point},
};

use crate::{
    emergency::EmergencyContext,
    hardware_bridge::HardwareBridge,
    overlay::OverlayControl,
    state::State,
};

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

fn deliver_pointer_motion(
    state: &mut State,
    location: Point<f64, Logical>,
    time: u32,
) {
    if state.overlay_open || state.context_menu.open || state.alt_tab.open {
        return;
    }
    state.pointer_pos = location;
    state.deliver_pointer_motion(time);
    // Redraw contínuo — antes só renderizava no clique ou no cursor do compositor (TTY).
    state.request_render_debounced(Duration::from_millis(8));
}

/// Simula clique direito na posição atual do cursor (só para debug).
pub fn debug_right_click(state: &mut State, tracker: &PointerTracker) {
    tracing::info!(
        "DEBUG: simulando clique direito em ({:.0}, {:.0})",
        tracker.pos.x,
        tracker.pos.y
    );
    let time = 0u32;
    state.deliver_pointer_motion(time);
    tracing::info!("DEBUG: botão direito press");
    state.deliver_pointer_button(273, ButtonState::Pressed, time);

    thread::sleep(Duration::from_millis(150));

    tracing::info!("DEBUG: botão direito release");
    state.deliver_pointer_button(273, ButtonState::Released, time);
}

pub fn handle_input<B: InputBackend>(
    state: &mut State,
    tracker: &mut PointerTracker,
    overlay: &OverlayControl,
    emergency: &EmergencyContext,
    hardware: &HardwareBridge,
    event: InputEvent<B>,
    tty_vt: Option<&mut crate::emergency::TtyVtControl<'_>>,
) {
    overlay.poll(state);
    emergency.menu.poll(state);
    hardware.set_pointer(tracker.pos.x, tracker.pos.y);

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
                        data, emergency, mods, keysym, key_state, tty_vt,
                    )
                },
            );
            if !state.overlay_open && !state.context_menu.open && !state.alt_tab.open {
                state.request_render();
            }
        }
        InputEvent::PointerMotion { event } => {
            let wm_ui = state.overlay_open || state.context_menu.open || state.alt_tab.open;
            let motion_scale = if wm_ui { 1.0 } else { speed };
            tracker.pos.x += event.delta_x() * motion_scale;
            tracker.pos.y += event.delta_y() * motion_scale;
            tracker.clamp(state.output_size);
            state.pointer_pos = tracker.pos;
            hardware.set_pointer(tracker.pos.x, tracker.pos.y);
            if state.overlay_open {
                if crate::settings::input::handle_pointer_motion(state, tracker.pos) {
                    state.request_render_debounced(Duration::from_millis(16));
                }
            } else if state.context_menu.open {
                if crate::context_menu::handlers::handle_pointer_motion(state, tracker.pos) {
                    state.request_render_debounced(Duration::from_millis(16));
                }
            } else {
                deliver_pointer_motion(state, tracker.pos, event.time() as u32);
            }
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let pos = Point::<f64, Logical>::from((event.x(), event.y()));
            tracker.pos = pos;
            tracker.clamp(state.output_size);
            state.pointer_pos = tracker.pos;
            hardware.set_pointer(tracker.pos.x, tracker.pos.y);
            if state.overlay_open {
                if crate::settings::input::handle_pointer_motion(state, tracker.pos) {
                    state.request_render_debounced(Duration::from_millis(16));
                }
            } else if state.context_menu.open {
                if crate::context_menu::handlers::handle_pointer_motion(state, tracker.pos) {
                    state.request_render_debounced(Duration::from_millis(16));
                }
            } else {
                deliver_pointer_motion(state, tracker.pos, event.time() as u32);
            }
        }
        InputEvent::PointerButton { event } => {
            const BTN_LEFT: u32 = 0x110;
            const BTN_RIGHT: u32 = 0x111;

            if state.overlay_open {
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

            if state.context_menu.open {
                if event.button_code() == BTN_LEFT {
                    let pressed = event.state() == ButtonState::Pressed;
                    if pressed {
                        crate::context_menu::handlers::handle_pointer_button(
                            state,
                            tracker.pos,
                            pressed,
                        );
                    }
                }
                return;
            }

            if state.focused_is_x11
                && state.x11_input_wanted
                && !state.x11_input_active()
                && event.button_code() == BTN_LEFT
                && event.state() == ButtonState::Pressed
            {
                state.apply_focus();
            }

            if event.button_code() == BTN_RIGHT
                && event.state() == ButtonState::Pressed
                && crate::context_menu::handlers::pointer_context_menu_modifier_held(state)
            {
                tracing::info!(
                    "Menu WM: clique direito em ({:.0}, {:.0})",
                    tracker.pos.x,
                    tracker.pos.y
                );
                crate::context_menu::handlers::open_at(state, tracker.pos);
                state.request_render();
                return;
            }
            let time = event.time() as u32;
            if event.button_code() == BTN_RIGHT {
                tracing::debug!(
                    "clique direito {:?} → cliente em ({:.0}, {:.0})",
                    event.state(),
                    state.pointer_pos.x,
                    state.pointer_pos.y,
                );
            } else {
                tracing::debug!(
                    "botão {} {:?} em ({:.0}, {:.0})",
                    event.button_code(),
                    event.state(),
                    state.pointer_pos.x,
                    state.pointer_pos.y
                );
            }
            state.deliver_pointer_button(event.button_code(), event.state(), time);
            state.request_render();
        }
        InputEvent::PointerAxis { event } => {
            if state.overlay_open || state.context_menu.open || state.alt_tab.open {
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
            state.request_render();
        }
        _ => {}
    }
}
