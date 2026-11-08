//! Bitmap page renderer (used for Windows printing). Same geometry as the PDF path.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::images::compose;
use crate::layout::{CROP_TICK_END, CROP_TICK_START, INCH, LineStyle, PT_PER_MM, TestPage};
use crate::pdf::{Document, Pages};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;

/// Render page `n` of `doc` as an opaque white bitmap at the given resolution (dots per inch, x and y).
pub fn render_page(doc: &Document, n: usize, dpi: (f32, f32)) -> RgbaImage {
    let sheet = doc.sheet;
    let (w_mm, h_mm) = sheet.page_mm();
    let ppm = (dpi.0 / INCH, dpi.1 / INCH);
    let (w, h) = (((w_mm * ppm.0).round() as u32).max(1), ((h_mm * ppm.1).round() as u32).max(1));
    let mut page = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    // mm → px, scaling about the page centre for calibration.
    let (sx, sy) = doc.calibration;
    let to_px = |(x, y): (f32, f32)| ((w_mm / 2.0 + sx * (x - w_mm / 2.0)) * ppm.0, (h_mm / 2.0 + sy * (y - h_mm / 2.0)) * ppm.1);
    let s_px = ppm.0 * sx; // px per mm after calibration (x axis)

    match doc.pages {
        Pages::Badges(pages) => {
            let items = pages.get(n).map_or(&[][..], Vec::as_slice);
            let cut_px = (sheet.cut_mm() * s_px).round() as u32;
            let lw = sheet.guides.weight_pt / PT_PER_MM * s_px;
            let mut badges: HashMap<usize, RgbaImage> = HashMap::new();
            for (i, cell) in sheet.cells().into_iter().enumerate() {
                let (cx, cy) = to_px(cell);
                if let Some(i) = items.get(i).copied().flatten()
                    && let Some(img) = doc.images.get(i)
                {
                    let badge = badges.entry(i).or_insert_with(|| compose(sheet, img, &img.full, cut_px));
                    let half = cut_px as f32 / 2.0;
                    image::imageops::overlay(&mut page, badge, (cx - half).round() as i64, (cy - half).round() as i64);
                } else if let Some(bg) = sheet.background {
                    disc(&mut page, cx, cy, sheet.cut_mm() / 2.0 * s_px, bg.map(|v| (v * 255.0) as u8));
                }
                guides(&mut page, doc, cx, cy, s_px, lw);
            }
        }
        Pages::TestPage => {
            let t = TestPage::new(sheet);
            for [a, b] in t.segments() {
                line(&mut page, to_px(a), to_px(b), (0.25 * s_px).max(1.0), [0, 0, 0]);
            }
            let (bx, by) = to_px(t.badge);
            for d in [sheet.cut_mm(), sheet.face_mm] {
                stroke_circle(&mut page, bx, by, d / 2.0 * s_px, LineStyle::Solid, 0.2 * s_px, [0, 0, 0]);
            }
        }
    }
    page
}

fn guides(page: &mut RgbaImage, doc: &Document, cx: f32, cy: f32, s_px: f32, lw: f32) {
    let g = &doc.sheet.guides;
    let v = (g.gray * 255.0) as u8;
    let col = [v, v, v];
    let cut_r = doc.sheet.cut_mm() / 2.0 * s_px;
    stroke_circle(page, cx, cy, cut_r, g.cut, lw, col);
    if g.crop_marks {
        let (a, b) = (cut_r + CROP_TICK_START * s_px, cut_r + CROP_TICK_END * s_px);
        line(page, (cx, cy - a), (cx, cy - b), lw, col);
        line(page, (cx, cy + a), (cx, cy + b), lw, col);
        line(page, (cx - a, cy), (cx - b, cy), lw, col);
        line(page, (cx + a, cy), (cx + b, cy), lw, col);
    }
}

/// Stroke a circle by stamping discs along it (dotted/dashed/solid patterns, same ratios as the PDF).
fn stroke_circle(page: &mut RgbaImage, cx: f32, cy: f32, r: f32, style: LineStyle, lw: f32, col: [u8; 3]) {
    let lw = lw.max(1.0);
    // (distance between stamps, "on" length, pattern period), all along the circumference.
    let (step, on, period) = match style {
        LineStyle::Dotted => (lw * 3.0, lw * 0.01, lw * 3.0),
        LineStyle::Dashed => (lw / 3.0, lw * 6.0, lw * 10.0),
        LineStyle::Solid => (lw / 3.0, f32::INFINITY, 1.0),
        LineStyle::None => return,
    };
    let mut d = 0.0;
    while d < std::f32::consts::TAU * r {
        if d % period <= on {
            let a = d / r;
            disc(page, cx + a.cos() * r, cy + a.sin() * r, lw / 2.0, col);
        }
        d += step;
    }
}

fn line(page: &mut RgbaImage, a: (f32, f32), b: (f32, f32), lw: f32, col: [u8; 3]) {
    let len = (b.0 - a.0).hypot(b.1 - a.1);
    let steps = (len / (lw / 3.0).max(0.5)).ceil().max(1.0);
    for i in 0..=steps as u32 {
        let t = i as f32 / steps;
        disc(page, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, lw / 2.0, col);
    }
}

/// Blend an anti-aliased filled circle onto the page.
fn disc(page: &mut RgbaImage, cx: f32, cy: f32, r: f32, col: [u8; 3]) {
    let r = r.max(0.5);
    let (x0, x1) = ((cx - r - 1.0).floor().max(0.0) as u32, ((cx + r + 1.0).ceil() as u32).min(page.width()));
    let (y0, y1) = ((cy - r - 1.0).floor().max(0.0) as u32, ((cy + r + 1.0).ceil() as u32).min(page.height()));
    for y in y0..y1 {
        for x in x0..x1 {
            let cov = (r + 0.5 - (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy)).clamp(0.0, 1.0);
            if cov > 0.0 {
                let p = page.get_pixel_mut(x, y);
                for k in 0..3 {
                    p[k] = (p[k] as f32 * (1.0 - cov) + col[k] as f32 * cov) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Guides, Sheet};

    #[test]
    fn cut_circle_diameter_in_pixels() {
        // A4 at 100 dpi with solid cut circles: measure the first one across its middle row.
        let sheet = Sheet { guides: Guides { cut: LineStyle::Solid, ..Default::default() }, ..Sheet::default() };
        let pages = vec![vec![None; sheet.cells().len()]];
        let doc = Document { sheet: &sheet, images: &[], pages: Pages::Badges(&pages), calibration: (1.0, 1.0), title: "t" };
        let page = render_page(&doc, 0, (100.0, 100.0));
        let (cx, cy) = sheet.cells()[0];
        let ppm = 100.0 / INCH;
        let row = (cy * ppm) as u32;
        // Ignore the neighbouring circles: only look within ~one radius of this centre.
        let dark: Vec<u32> = (0..page.width())
            .filter(|&x| page.get_pixel(x, row)[0] < 200 && (x as f32 - cx * ppm).abs() < sheet.cut_mm() * ppm * 0.52)
            .collect();
        let measured_mm = (dark.last().unwrap() - dark[0]) as f32 / ppm;
        assert!((measured_mm - sheet.cut_mm()).abs() < 1.0, "measured {measured_mm} vs {}", sheet.cut_mm());
    }
}
