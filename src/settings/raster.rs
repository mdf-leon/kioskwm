use crate::font8x8::glyph;

use super::theme::Rgba;

pub struct Canvas {
    pub pixels: Vec<u8>,
    pub w: i32,
    pub h: i32,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Self {
            pixels: vec![0u8; (w * h * 4) as usize],
            w,
            h,
        }
    }

    pub fn fill(&mut self, color: Rgba) {
        for y in 0..self.h {
            for x in 0..self.w {
                self.put(x, y, color);
            }
        }
    }

    pub fn put(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        if i + 3 < self.pixels.len() {
            self.pixels[i] = c.b;
            self.pixels[i + 1] = c.g;
            self.pixels[i + 2] = c.r;
            self.pixels[i + 3] = c.a;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for dy in 0..h {
            for dx in 0..w {
                self.put(x + dx, y + dy, c);
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Rgba) {
        let r = r.min(w / 2).min(h / 2).max(0);
        for dy in 0..h {
            for dx in 0..w {
                let mut inside = true;
                if dx < r && dy < r {
                    inside = (dx - r) * (dx - r) + (dy - r) * (dy - r) <= r * r;
                } else if dx >= w - r && dy < r {
                    inside = (dx - (w - r - 1)) * (dx - (w - r - 1)) + (dy - r) * (dy - r) <= r * r;
                } else if dx < r && dy >= h - r {
                    inside = (dx - r) * (dx - r) + (dy - (h - r - 1)) * (dy - (h - r - 1)) <= r * r;
                } else if dx >= w - r && dy >= h - r {
                    inside = (dx - (w - r - 1)) * (dx - (w - r - 1))
                        + (dy - (h - r - 1)) * (dy - (h - r - 1))
                        <= r * r;
                }
                if inside {
                    self.put(x + dx, y + dy, c);
                }
            }
        }
    }

    /// Borda fina estilo Breeze (preenchimento + recorte interno).
    pub fn bordered_rounded_rect(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: i32,
        fill: Rgba,
        border: Rgba,
    ) {
        self.fill_rounded_rect(x, y, w, h, r, border);
        self.fill_rounded_rect(x + 1, y + 1, w - 2, h - 2, (r - 1).max(0), fill);
    }

    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: Rgba) {
        self.fill_rect(x, y, w, 1, c);
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, c: Rgba, scale: i32) {
        let mut cx = x;
        for ch in s.chars() {
            let bmp = glyph(ch);
            for (row, bits) in bmp.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.put(
                                    cx + col * scale + sx,
                                    y + row as i32 * scale + sy,
                                    c,
                                );
                            }
                        }
                    }
                }
            }
            cx += 8 * scale + scale / 2;
        }
    }

    pub fn text_width(s: &str, scale: i32) -> i32 {
        if s.is_empty() {
            return 0;
        }
        s.chars().count() as i32 * (8 * scale + scale / 2) - scale / 2
    }

    pub fn draw_hamburger(&mut self, x: i32, y: i32, c: Rgba) {
        for i in 0..3 {
            self.fill_rounded_rect(x, y + i * 7, 20, 3, 1, c);
        }
    }

    /// Icone mouse estilo Breeze (silhueta clara).
    pub fn draw_breeze_mouse_icon(&mut self, x: i32, y: i32) {
        let body = Rgba::new(161, 169, 177, 255);
        let hi = Rgba::new(220, 224, 228, 255);
        self.fill_rounded_rect(x + 6, y + 2, 14, 22, 6, body);
        self.fill_rounded_rect(x + 9, y + 5, 3, 8, 1, hi);
        self.fill_rounded_rect(x + 14, y + 5, 3, 8, 1, hi);
        self.fill_rect(x + 11, y + 2, 2, 4, hi);
        self.fill_rounded_rect(x + 9, y + 24, 8, 6, 2, body);
    }
}
