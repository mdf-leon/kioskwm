use crate::state::State;

pub const TILE_W: i32 = 148;
pub const TILE_H: i32 = 128;
pub const TILE_GAP: i32 = 14;
pub const ICON_SIZE: i32 = 72;
pub const PANEL_PAD: i32 = 24;
pub const ROW_RADIUS: i32 = 10;

pub const BLINK_MS: u32 = 220;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub fn row_size(count: usize) -> (i32, i32) {
    if count == 0 {
        return (0, 0);
    }
    let n = count as i32;
    let w = n * TILE_W + (n - 1) * TILE_GAP + PANEL_PAD * 2;
    let h = TILE_H + PANEL_PAD * 2;
    (w, h)
}

pub fn row_origin(output_w: i32, output_h: i32, count: usize) -> (i32, i32) {
    let (rw, rh) = row_size(count);
    let x = (output_w - rw) / 2;
    let y = (output_h - rh) / 2;
    (x.max(0), y.max(0))
}

pub fn tile_rect(row_x: i32, row_y: i32, index: usize) -> Rect {
    let x = row_x + PANEL_PAD + index as i32 * (TILE_W + TILE_GAP);
    let y = row_y + PANEL_PAD;
    Rect {
        x,
        y,
        w: TILE_W,
        h: TILE_H,
    }
}

pub fn icon_rect(tile: Rect) -> Rect {
    let x = tile.x + (tile.w - ICON_SIZE) / 2;
    let y = tile.y + 12;
    Rect {
        x,
        y,
        w: ICON_SIZE,
        h: ICON_SIZE,
    }
}

pub fn blink_phase(time_ms: u32) -> bool {
    (time_ms / BLINK_MS) % 2 == 0
}

pub fn app_icon_letter(name: &str) -> char {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?')
}

pub fn ordered_indices(state: &State) -> Vec<usize> {
    let count = state.app_count();
    if count == 0 {
        return Vec::new();
    }
    let current = state.unified_focus_index().min(count - 1);
    let mut order = state.app_mru.clone();
    order.retain(|&i| i < count);
    for i in 0..count {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    if let Some(pos) = order.iter().position(|&i| i == current) {
        let tail = order.split_off(pos);
        order = tail.into_iter().chain(order).collect();
    } else {
        order.insert(0, current);
    }
    order
}
