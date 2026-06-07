use smithay::{
    backend::input::KeyState,
    input::keyboard::{FilterResult, KeysymHandle, ModifiersState},
};
use xkbcommon::xkb::keysyms;

use crate::state::State;

use super::{layout, render::invalidate_cache};

fn is_alt(keysym: &KeysymHandle<'_>) -> bool {
    keysym
        .raw_syms()
        .iter()
        .any(|s| matches!(s.raw(), keysyms::KEY_Alt_L | keysyms::KEY_Alt_R))
}

fn is_tab(keysym: &KeysymHandle<'_>) -> bool {
    keysym.modified_sym().raw() == keysyms::KEY_Tab as u32
}

pub fn keyboard_filter(
    state: &mut State,
    modifiers: &ModifiersState,
    keysym: &KeysymHandle<'_>,
    key_state: KeyState,
) -> FilterResult<()> {
    if is_alt(keysym) {
        if key_state == KeyState::Released {
            confirm(state);
        }
        return FilterResult::Intercept(());
    }

    if key_state == KeyState::Pressed && modifiers.alt && is_tab(keysym) {
        if modifiers.shift {
            cycle_prev(state);
        } else {
            cycle_next(state);
        }
        return FilterResult::Intercept(());
    }

    if state.alt_tab.open {
        if key_state == KeyState::Pressed
            && keysym.modified_sym().raw() == keysyms::KEY_Escape as u32
        {
            close(state);
        }
        return FilterResult::Intercept(());
    }

    FilterResult::Forward
}

pub fn try_open(state: &mut State, modifiers: &ModifiersState, keysym: &KeysymHandle<'_>, key_state: KeyState) -> bool {
    if key_state != KeyState::Pressed || !modifiers.alt || modifiers.ctrl || modifiers.logo {
        return false;
    }
    if !is_tab(keysym) {
        return false;
    }
    if modifiers.shift {
        cycle_prev(state);
    } else {
        cycle_next(state);
    }
    true
}

fn open_overlay(state: &mut State) {
    state.alt_tab.open = true;
    state.deferred_focus = true;
    state.suspend_client_keyboard_for_wm_ui();
    state.note_full_damage();
    state.request_render();
}

fn cycle_next(state: &mut State) {
    state.sync_app_mru();
    let count = state.app_count();
    if count <= 1 {
        return;
    }
    if !state.alt_tab.open {
        open_overlay(state);
        state.alt_tab.slot = 1;
    } else {
        state.alt_tab.slot = (state.alt_tab.slot + 1) % count;
    }
    invalidate_cache(state);
    state.request_render();
}

fn cycle_prev(state: &mut State) {
    state.sync_app_mru();
    let count = state.app_count();
    if count <= 1 {
        return;
    }
    if !state.alt_tab.open {
        open_overlay(state);
        state.alt_tab.slot = count - 1;
    } else {
        state.alt_tab.slot = (state.alt_tab.slot + count - 1) % count;
    }
    invalidate_cache(state);
    state.request_render();
}

fn confirm(state: &mut State) {
    if !state.alt_tab.open {
        return;
    }
    let order = layout::ordered_indices(state);
    let count = order.len();
    if count == 0 {
        close(state);
        return;
    }
    let slot = state.alt_tab.slot.min(count - 1);
    let idx = order[slot];
    let name = state.unified_app_name(idx).to_string();

    state.alt_tab.open = false;
    state.alt_tab.slot = 0;
    invalidate_cache(state);

    tracing::info!("Alt+Tab confirma índice {idx} → {name}");
    state.focus_unified(idx);
    state.resync_input_after_overlay();
    state.request_render();
}

pub fn close(state: &mut State) {
    if !state.alt_tab.open {
        return;
    }
    state.alt_tab.open = false;
    state.alt_tab.slot = 0;
    invalidate_cache(state);
    state.resync_input_after_overlay();
    state.request_render();
}
