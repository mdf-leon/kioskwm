use smithay::{
    backend::input::KeyState,
    input::keyboard::{FilterResult, KeysymHandle, ModifiersState},
    utils::Point,
};
use xkbcommon::xkb::keysyms;

use crate::state::State;

use super::{
    actions::execute_confirm,
    layout::{self, ConfirmAction, Hit, Screen},
    render::invalidate_cache,
    slider::{self, SPEED_MAX, SPEED_MIN},
};

pub fn handle_pointer_button(state: &mut State, pos: Point<f64, smithay::utils::Logical>, pressed: bool) {
    if !state.overlay_open || !pressed {
        return;
    }
    let Some((lx, ly)) = layout::pointer_to_panel_local(
        pos.x,
        pos.y,
        state.output_size.w,
        state.output_size.h,
    ) else {
        return;
    };

    match layout::hit_test(state.settings.screen, state.settings.confirm, lx, ly) {
        Hit::None => {}
        Hit::Close => {
            state.overlay_open = false;
            reset_on_open(state);
        }
        Hit::AppletMouse => {
            state.settings.screen = Screen::Mouse;
            invalidate_cache(state);
        }
        Hit::FooterQuit => {
            state.settings.confirm = Some(ConfirmAction::QuitWm);
            invalidate_cache(state);
        }
        Hit::FooterShutdown => {
            state.settings.confirm = Some(ConfirmAction::Shutdown);
            invalidate_cache(state);
        }
        Hit::FooterReboot => {
            state.settings.confirm = Some(ConfirmAction::Reboot);
            invalidate_cache(state);
        }
        Hit::MouseBack => {
            state.settings.screen = Screen::Main;
            state.settings.slider_drag = false;
            invalidate_cache(state);
        }
        Hit::Slider => {
            state.settings.slider_drag = true;
            apply_speed(state, layout::slider_value_from_x(lx));
        }
        Hit::ConfirmCancel => {
            state.settings.confirm = None;
            invalidate_cache(state);
        }
        Hit::ConfirmOk => {
            if let Some(action) = state.settings.confirm.take() {
                execute_confirm(state, action);
            }
            invalidate_cache(state);
        }
    }
}

pub fn handle_pointer_motion(state: &mut State, pos: Point<f64, smithay::utils::Logical>) {
    if !state.overlay_open {
        return;
    }

    let hit = layout::pointer_to_panel_local(
        pos.x,
        pos.y,
        state.output_size.w,
        state.output_size.h,
    )
    .map(|(lx, ly)| layout::hit_test(state.settings.screen, state.settings.confirm, lx, ly))
    .unwrap_or(Hit::None);

    let hover = (hit != Hit::None).then_some(hit);
    if state.settings.hover != hover {
        state.settings.hover = hover;
        invalidate_cache(state);
    }

    if state.settings.slider_drag
        && state.settings.confirm.is_none()
        && state.settings.screen == Screen::Mouse
    {
        let Some((lx, _)) = layout::pointer_to_panel_local(
            pos.x,
            pos.y,
            state.output_size.w,
            state.output_size.h,
        ) else {
            return;
        };
        apply_speed(state, layout::slider_value_from_x(lx));
    }
}

pub fn handle_pointer_release(state: &mut State) {
    state.settings.slider_drag = false;
}

fn apply_speed(state: &mut State, speed: f64) {
    let q = (speed * 20.0).round() / 20.0;
    let q = q.clamp(SPEED_MIN, SPEED_MAX);
    if (state.pointer_speed - q).abs() < 0.0001 {
        return;
    }
    state.pointer_speed = q;
    invalidate_cache(state);
}

pub fn keyboard_filter(
    state: &mut State,
    _mods: &ModifiersState,
    keysym: &KeysymHandle<'_>,
    key_state: KeyState,
) -> FilterResult<()> {
    if key_state != KeyState::Pressed {
        return FilterResult::Intercept(());
    }

    let sym = keysym.modified_sym().raw();

    if state.settings.confirm.is_some() {
        match sym {
            k if k == keysyms::KEY_Escape as u32 => {
                state.settings.confirm = None;
                invalidate_cache(state);
            }
            k if k == keysyms::KEY_Return as u32 || k == keysyms::KEY_KP_Enter as u32 => {
                if let Some(action) = state.settings.confirm.take() {
                    execute_confirm(state, action);
                }
                invalidate_cache(state);
            }
            _ => {}
        }
        return FilterResult::Intercept(());
    }

    match sym {
        k if k == keysyms::KEY_Escape as u32 => {
            match state.settings.screen {
                Screen::Main => state.overlay_open = false,
                Screen::Mouse => {
                    state.settings.screen = Screen::Main;
                    invalidate_cache(state);
                }
            }
        }
        k if k == keysyms::KEY_BackSpace as u32 && state.settings.screen == Screen::Mouse => {
            state.settings.screen = Screen::Main;
            invalidate_cache(state);
        }
        _ if state.settings.screen == Screen::Mouse => {
            adjust_speed_key(state, sym);
        }
        _ => {}
    }

    FilterResult::Intercept(())
}

fn adjust_speed_key(state: &mut State, sym: u32) {
    let factor = match sym {
        k if k == keysyms::KEY_Left as u32
            || k == keysyms::KEY_Down as u32
            || k == keysyms::KEY_minus as u32
            || k == keysyms::KEY_KP_Subtract as u32 =>
        {
            0.9
        }
        k if k == keysyms::KEY_Right as u32
            || k == keysyms::KEY_Up as u32
            || k == keysyms::KEY_plus as u32
            || k == keysyms::KEY_equal as u32
            || k == keysyms::KEY_KP_Add as u32 =>
        {
            1.1
        }
        _ => return,
    };
    apply_speed(state, (state.pointer_speed * factor).clamp(SPEED_MIN, SPEED_MAX));
}

pub fn reset_on_open(state: &mut State) {
    state.settings.screen = Screen::Main;
    state.settings.confirm = None;
    state.settings.slider_drag = false;
    state.settings.hover = None;
    if state.pointer_speed < SPEED_MIN || state.pointer_speed > SPEED_MAX {
        state.pointer_speed = slider::SPEED_CENTER;
    }
    invalidate_cache(state);
}
