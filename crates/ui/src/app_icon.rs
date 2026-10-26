//! The Mailwright mark, drawn in code: an envelope on a rounded square in the app colour, with a
//! small calendar tab. The packaged icons (assets/app-icon) are rendered from the same design.

use egui::{Color32, CornerRadius, Painter, Rect, Shape, Stroke, pos2};

use crate::theme::APP_COLOR;

pub fn paint(p: &Painter, r: Rect) {
    let s = r.width().min(r.height()) / 20.0;
    let o = r.min;
    let pt = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    p.rect_filled(Rect::from_min_max(pt(0.0, 0.0), pt(20.0, 20.0)), CornerRadius::same((4.0 * s) as u8), APP_COLOR);
    let white = Color32::WHITE;
    let env = Rect::from_min_max(pt(3.5, 5.5), pt(16.5, 14.5));
    p.rect_filled(env, CornerRadius::same((1.2 * s) as u8), white);
    p.add(Shape::line(vec![pt(4.2, 6.3), pt(10.0, 10.8), pt(15.8, 6.3)], Stroke::new(1.3 * s, APP_COLOR)));
    // The calendar tab, bottom right.
    let tab = Rect::from_min_max(pt(12.0, 12.0), pt(18.0, 18.0));
    p.rect_filled(tab, CornerRadius::same((1.0 * s) as u8), Color32::from_rgb(0x0B, 0x4F, 0x8C));
    p.rect_filled(Rect::from_min_max(pt(12.0, 12.0), pt(18.0, 13.8)), CornerRadius::same((1.0 * s) as u8), Color32::from_rgb(0x8F, 0xD3, 0xFF));
}

/// The icon as RGBA pixels (`size` square), for the window icon.
pub fn rgba(size: u32) -> Vec<u8> {
    draw(size, false)
}

/// The icon with a new-mail badge (an amber disc with a small envelope, top right), for the
/// taskbar while new mail is waiting.
pub fn rgba_badged(size: u32) -> Vec<u8> {
    draw(size, true)
}

fn draw(size: u32, badge: bool) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32 / 20.0;
    let blue = [0x0F, 0x6C, 0xBD];
    let dark = [0x0B, 0x4F, 0x8C];
    let light = [0x8F, 0xD3, 0xFF];
    let amber = [0xE8, 0x8A, 0x00];
    let in_round = |x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, rad: f32| {
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return false;
        }
        let cx = x.clamp(x0 + rad, x1 - rad);
        let cy = y.clamp(y0 + rad, y1 - rad);
        (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad
    };
    let near_line = |x: f32, y: f32, a: (f32, f32), b: (f32, f32), w: f32| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (px, py) = (a.0 + t * dx, a.1 + t * dy);
        (x - px).powi(2) + (y - py).powi(2) <= (w / 2.0).powi(2)
    };
    for yy in 0..size {
        for xx in 0..size {
            // Four samples per pixel for smooth edges.
            let mut acc = [0f32; 4];
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let x = (xx as f32 + sx) / s;
                let y = (yy as f32 + sy) / s;
                let mut c: Option<[u8; 3]> = None;
                if in_round(x, y, 0.0, 0.0, 20.0, 20.0, 4.0) {
                    c = Some(blue);
                }
                if in_round(x, y, 3.5, 5.5, 16.5, 14.5, 1.2) {
                    c = Some([255, 255, 255]);
                    if near_line(x, y, (4.2, 6.3), (10.0, 10.8), 1.3) || near_line(x, y, (10.0, 10.8), (15.8, 6.3), 1.3) {
                        c = Some(blue);
                    }
                }
                if in_round(x, y, 12.0, 12.0, 18.0, 18.0, 1.0) {
                    c = Some(if y < 13.8 { light } else { dark });
                }
                if badge {
                    let d2 = (x - 15.2).powi(2) + (y - 4.8).powi(2);
                    if d2 <= 4.8f32.powi(2) {
                        c = Some([255, 255, 255]);
                    }
                    if d2 <= 4.0f32.powi(2) {
                        c = Some(amber);
                        if in_round(x, y, 12.9, 3.2, 17.5, 6.6, 0.4) {
                            c = Some([255, 255, 255]);
                            if near_line(x, y, (13.1, 3.5), (15.2, 5.1), 0.55) || near_line(x, y, (15.2, 5.1), (17.3, 3.5), 0.55) {
                                c = Some(amber);
                            }
                        }
                    }
                }
                if let Some(c) = c {
                    acc[0] += c[0] as f32;
                    acc[1] += c[1] as f32;
                    acc[2] += c[2] as f32;
                    acc[3] += 255.0;
                }
            }
            let i = ((yy * size + xx) * 4) as usize;
            let a = acc[3] / 4.0;
            if a > 0.0 {
                let k = 255.0 / acc[3];
                px[i] = (acc[0] * k) as u8;
                px[i + 1] = (acc[1] * k) as u8;
                px[i + 2] = (acc[2] * k) as u8;
                px[i + 3] = a as u8;
            }
        }
    }
    px
}

/// Write the icon as a PNG (for packaging).
pub fn png(size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_raw(size, size, rgba(size)).unwrap_or_default();
    let _ = image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    out
}
