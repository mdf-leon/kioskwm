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

    pub fn draw_hamburger(&mut self, x: i32, y: i32, c: Rgba) {
        for i in 0..3 {
            self.fill_rounded_rect(x, y + i * 6, 18, 2, 1, c);
        }
    }

    pub fn draw_close(&mut self, x: i32, y: i32, c: Rgba) {
        for i in 0..12 {
            self.put(x + i, y + i, c);
            self.put(x + 11 - i, y + i, c);
        }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, fill: Rgba, border: Rgba) {
        let r = r.max(1);
        for dy in -r..=r {
            for dx in -r..=r {
                let d2 = dx * dx + dy * dy;
                if d2 <= r * r {
                    let c = if d2 >= (r - 1) * (r - 1) { border } else { fill };
                    self.put(cx + dx, cy + dy, c);
                }
            }
        }
    }
}
