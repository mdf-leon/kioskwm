use smithay::{
    backend::input::KeyState,
    input::keyboard::{FilterResult, KeysymHandle, ModifiersState},
    utils::Point,
};
use xkbcommon::xkb::keysyms;

use crate::{
    settings::input::reset_on_open,
    state::State,
};

use super::{
    layout::{self, Hit},
    render::invalidate_cache,
};

pub fn super_held(state: &State) -> bool {
    crate::modifiers::super_held(
        state.keyboard.modifier_state().logo,
        state.local_super_keys,
        &state.mod_tracker,
    )
}

pub fn right_alt_held(state: &State) -> bool {
    crate::modifiers::right_alt_held(state.local_right_alt_keys, &state.mod_tracker)
}

pub fn context_menu_modifier_held(state: &State) -> bool {
    crate::modifiers::context_menu_modifier_held(
        state.keyboard.modifier_state().logo,
        state.local_super_keys,
        state.local_right_alt_keys,
        &state.mod_tracker,
    )
}

/// Modificadores para menu WM via mouse — evdev só Super (TTY/aninhado), nunca Alt evdev.
pub fn pointer_context_menu_modifier_held(state: &State) -> bool {
    let no_evdev = crate::modifiers::ModifierTracker::default();
    if crate::modifiers::context_menu_modifier_held(
        state.keyboard.modifier_state().logo,
        state.local_super_keys,
        state.local_right_alt_keys,
        &no_evdev,
    ) {
        return true;
    }
    // Super físico via evdev (KDE engole teclas no winit); Alt evdev excluído (falso positivo).
    state.mod_tracker.evdev_super()
}

pub fn open_at_logical(state: &mut State, x: f64, y: f64) {
    let (ox, oy) = layout::menu_origin(
        x,
        y,
        state.output_size.w,
        state.output_size.h,
        state.app_count(),
    );
    state.context_menu.open = true;
    state.context_menu.origin_x = ox;
    state.context_menu.origin_y = oy;
    let hit = layout::hit_test(state, ox, oy, x, y);
    state.context_menu.hover = match hit {
        Hit::None => None,
        other => Some(other),
    };
    invalidate_cache(state);
    state.suspend_client_keyboard_for_wm_ui();
    state.invalidate_wm_backdrop();
    state.note_full_damage();
    state.request_render();
    let count = state.app_count();
    tracing::info!("Menu WM em ({ox}, {oy}) — {count} app(s), foco índice {}", state.unified_focus_index());
    for i in 0..count {
        let mark = if i == state.unified_focus_index() { " ●" } else { "" };
        tracing::info!("  [{i}] {}{}", state.unified_app_name(i), mark);
    }
}

pub fn open_at(state: &mut State, pos: Point<f64, smithay::utils::Logical>) {
    open_at_logical(state, pos.x, pos.y);
}

pub fn close(state: &mut State) {
    if !state.context_menu.open {
        return;
    }
    state.context_menu.open = false;
    state.context_menu.hover = None;
    invalidate_cache(state);
    state.clear_wm_backdrop();
    state.resync_input_after_overlay();
    state.request_render();
}

pub fn handle_pointer_button(
    state: &mut State,
    pos: Point<f64, smithay::utils::Logical>,
    pressed: bool,
) {
    if !state.context_menu.open || !pressed {
        return;
    }

    let hit = layout::hit_test(
        state,
        state.context_menu.origin_x,
        state.context_menu.origin_y,
        pos.x,
        pos.y,
    );
    match hit {
        Hit::App(i) => {
            state.focus_unified(i);
            close(state);
        }
        Hit::CloseApp => {
            close(state);
            state.close_focused();
        }
        Hit::OpenSettings => {
            close(state);
            state.overlay_open = true;
            reset_on_open(state);
            state.invalidate_wm_backdrop();
            state.request_render();
        }
        Hit::None => close(state),
    }
}

pub fn handle_pointer_motion(state: &mut State, pos: Point<f64, smithay::utils::Logical>) -> bool {
    if !state.context_menu.open {
        return false;
    }

    let hit = layout::hit_test(
        state,
        state.context_menu.origin_x,
        state.context_menu.origin_y,
        pos.x,
        pos.y,
    );
    let hover = match hit {
        Hit::None => None,
        other => Some(other),
    };
    if state.context_menu.hover != hover {
        state.context_menu.hover = hover;
        invalidate_cache(state);
        return true;
    }
    false
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

    if keysym.modified_sym().raw() == keysyms::KEY_Escape as u32 {
        close(state);
    }

    FilterResult::Intercept(())
}
