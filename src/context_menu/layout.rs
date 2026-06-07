use crate::{apps, i18n::I18n, state::State};

use crate::settings::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    None,
    App(usize),
    CloseApp,
    OpenSettings,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

pub const MENU_W: i32 = 260;
pub const ITEM_H: i32 = 32;
pub const PADDING: i32 = 4;
pub const RADIUS: i32 = 6;
pub const HEADER_H: i32 = 28;
pub const DIVIDER_H: i32 = 1;

fn close_section_h(app_count: usize) -> i32 {
    if app_count > 0 {
        ITEM_H + DIVIDER_H
    } else {
        0
    }
}

pub fn menu_size(app_count: usize) -> (i32, i32) {
    let apps_h = app_count as i32 * ITEM_H;
    let close_h = close_section_h(app_count);
    let settings_h = ITEM_H;
    let h = PADDING * 2 + HEADER_H + apps_h + DIVIDER_H + close_h + settings_h;
    (MENU_W, h.max(PADDING * 2 + HEADER_H + ITEM_H + DIVIDER_H + ITEM_H))
}

pub fn menu_origin(anchor_x: f64, anchor_y: f64, output_w: i32, output_h: i32, app_count: usize) -> (i32, i32) {
    let (mw, mh) = menu_size(app_count);
    let mut x = anchor_x.floor() as i32;
    let mut y = anchor_y.floor() as i32;
    if x + mw > output_w {
        x = output_w - mw;
    }
    if y + mh > output_h {
        y = output_h - mh;
    }
    (x.max(0), y.max(0))
}

pub fn header_rect(origin_x: i32, origin_y: i32) -> Rect {
    Rect {
        x: origin_x + PADDING,
        y: origin_y + PADDING,
        w: MENU_W - PADDING * 2,
        h: HEADER_H,
    }
}

pub fn app_rect(origin_x: i32, origin_y: i32, index: usize) -> Rect {
    let y = origin_y + PADDING + HEADER_H + index as i32 * ITEM_H;
    Rect {
        x: origin_x + PADDING + 8,
        y,
        w: MENU_W - PADDING * 2 - 8,
        h: ITEM_H,
    }
}

pub fn close_rect(origin_x: i32, origin_y: i32, app_count: usize) -> Rect {
    let y = origin_y + PADDING + HEADER_H + app_count as i32 * ITEM_H + DIVIDER_H;
    Rect {
        x: origin_x + PADDING,
        y,
        w: MENU_W - PADDING * 2,
        h: ITEM_H,
    }
}

pub fn settings_rect(origin_x: i32, origin_y: i32, app_count: usize) -> Rect {
    let y = close_rect(origin_x, origin_y, app_count).y + close_section_h(app_count);
    Rect {
        x: origin_x + PADDING,
        y,
        w: MENU_W - PADDING * 2,
        h: ITEM_H,
    }
}

pub fn divider_y(origin_y: i32, app_count: usize) -> i32 {
    origin_y + PADDING + HEADER_H + app_count as i32 * ITEM_H
}

/// Coordenadas locais ao menu — mesma origem do canvas (Y+ para baixo, como o painel P1).
pub fn pointer_to_menu_local(
    _state: &State,
    origin_x: i32,
    origin_y: i32,
    px: f64,
    py: f64,
) -> Option<(i32, i32)> {
    let (mw, mh) = menu_size(_state.app_count());
    let lx_local = px.floor() as i32 - origin_x;
    let ly_local = py.floor() as i32 - origin_y;
    if lx_local >= 0 && ly_local >= 0 && lx_local < mw && ly_local < mh {
        Some((lx_local, ly_local))
    } else {
        None
    }
}

pub fn hit_test(state: &State, origin_x: i32, origin_y: i32, px: f64, py: f64) -> Hit {
    let app_count = state.app_count();
    let Some((lx, ly)) = pointer_to_menu_local(state, origin_x, origin_y, px, py) else {
        return Hit::None;
    };
    for i in 0..app_count {
        if app_rect(0, 0, i).contains(lx, ly) {
            tracing::trace!(
                "Menu hit [{i}]={} local ({lx},{ly}) ptr=({px:.0},{py:.0})",
                state.unified_app_name(i),
            );
            return Hit::App(i);
        }
    }
    if app_count > 0 && close_rect(0, 0, app_count).contains(lx, ly) {
        return Hit::CloseApp;
    }
    if settings_rect(0, 0, app_count).contains(lx, ly) {
        Hit::OpenSettings
    } else {
        Hit::None
    }
}

pub fn item_bg(hover: Option<Hit>, hit: Hit) -> theme::Rgba {
    if hover == Some(hit) {
        theme::BUTTON_HOVER
    } else {
        theme::WINDOW_BG
    }
}

pub fn app_label(i18n: I18n, app: &apps::RunningApp, focused: bool) -> String {
    unified_label(i18n, &app.display_name, focused)
}

pub fn unified_label(i18n: I18n, name: &str, focused: bool) -> String {
    let main = i18n.t(crate::i18n::Msg::Main);
    apps::menu_label(main, name) + if focused { "  ●" } else { "" }
}
