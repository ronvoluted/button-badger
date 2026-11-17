//! Bitmap page renderer (used for Windows printing). Same geometry as the PDF path: everything is drawn in mm
//! through one mm → pixel transform, in horizontal bands so a page is never held in memory whole.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::images::compose;
use crate::layout::{CROP_TICK_END, CROP_TICK_START, INCH, LineStyle, PT_PER_MM, TestPage};
use crate::pdf::{Document, Pages};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use resvg::tiny_skia::{FillRule, LineCap, Paint, PathBuilder, PixmapMut, Stroke, StrokeDash, Transform};
use std::collections::HashMap;

/// Composited badge per image index. Keep one for the whole print job so each image is composed once.
pub type Badges = HashMap<usize, RgbaImage>;

/// Page size in pixels at `dpi` (dots per inch, x and y).
pub fn page_px(doc: &Document, dpi: (f32, f32)) -> (u32, u32) {
    let (w_mm, h_mm) = doc.sheet.page_mm();
    (((w_mm * dpi.0 / INCH).round() as u32).max(1), ((h_mm * dpi.1 / INCH).round() as u32).max(1))
}

/// Render pixel rows `y0..y0 + rows` of page `n` as an opaque white-backed bitmap at `dpi`.
pub fn render_band(doc: &Document, n: usize, dpi: (f32, f32), y0: u32, rows: u32, badges: &mut Badges) -> RgbaImage {
    let sheet = doc.sheet;
    let (w_mm, h_mm) = sheet.page_mm();
    let mut band = RgbaImage::from_pixel(page_px(doc, dpi).0, rows.max(1), Rgba([255; 4]));
    // Pixels per mm on each axis. The printer's resolution and the calibration can both differ between x and y.
    let (kx, ky) = (dpi.0 / INCH * doc.calibration.0, dpi.1 / INCH * doc.calibration.1);
    // mm → page pixels, scaling about the page centre like the PDF's page transform. `at` is the same for a point.
    let (tx, ty) = (w_mm / 2.0 * (dpi.0 / INCH - kx), h_mm / 2.0 * (dpi.1 / INCH - ky));
    let t = Transform::from_row(kx, 0.0, 0.0, ky, tx, ty - y0 as f32);
    let hair = 1.0 / kx.min(ky); // one pixel in mm: no line is drawn thinner
    let cut_r = sheet.cut_mm() / 2.0;

    match doc.pages {
        Pages::Badges(pages) => {
            let items = pages.get(n).map_or(&[][..], Vec::as_slice);
            let (bw, bh) = (((sheet.cut_mm() * kx).round() as u32).max(2), ((sheet.cut_mm() * ky).round() as u32).max(2));
            let reach = (cut_r + CROP_TICK_END + 1.0) * ky; // how far a cell draws above and below its centre, px
            for (i, (cx, cy)) in sheet.cells().into_iter().enumerate() {
                let (px, py) = (kx * cx + tx, ky * cy + ty);
                if py + reach < y0 as f32 || py - reach > (y0 + rows) as f32 {
                    continue;
                }
                if let Some(i) = items.get(i).copied().flatten()
                    && let Some(img) = doc.images.get(i)
                {
                    let badge = badges.entry(i).or_insert_with(|| {
                        // `compose` makes square badges; squash to the device's pixel shape when the axes differ.
                        let square = compose(sheet, img, &img.full, bw.max(bh));
                        if bw == bh { square } else { imageops::resize(&square, bw, bh, FilterType::Triangle) }
                    });
                    let (x, y) = ((px - bw as f32 / 2.0).round() as i64, (py - bh as f32 / 2.0).round() as i64);
                    imageops::overlay(&mut band, badge, x, y - y0 as i64);
                } else if let (Some(bg), Some(disc)) = (sheet.background, PathBuilder::from_circle(cx, cy, cut_r)) {
                    canvas(&mut band).fill_path(&disc, &paint(bg.map(|v| (v * 255.0) as u8)), FillRule::Winding, t, None);
                }
                guides(&mut band, t, doc, cx, cy, hair);
            }
        }
        Pages::TestPage => {
            let test = TestPage::new(sheet);
            let mut rules = PathBuilder::new();
            for [a, b] in test.segments() {
                rules.move_to(a.0, a.1);
                rules.line_to(b.0, b.1);
            }
            let black = paint([0; 3]);
            let mut c = canvas(&mut band);
            if let (Some(rules), Some(s)) = (rules.finish(), stroke(LineStyle::Solid, 0.25f32.max(hair))) {
                c.stroke_path(&rules, &black, &s, t, None);
            }
            // Both circles solid and thin so they can be measured, whatever the guide style.
            let (bx, by) = test.badge;
            for d in [sheet.cut_mm(), sheet.face_mm] {
                if let (Some(circle), Some(s)) = (PathBuilder::from_circle(bx, by, d / 2.0), stroke(LineStyle::Solid, 0.2f32.max(hair))) {
                    c.stroke_path(&circle, &black, &s, t, None);
                }
            }
        }
    }
    band
}

/// The band as a tiny-skia canvas. It is opaque, so its straight RGBA is also valid premultiplied RGBA.
fn canvas(band: &mut RgbaImage) -> PixmapMut<'_> {
    let (w, h) = band.dimensions();
    PixmapMut::from_bytes(band, w, h).expect("an RGBA buffer of its own size")
}

fn paint(col: [u8; 3]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(col[0], col[1], col[2], 255);
    p
}

/// Stroke for a guide line `lw` mm wide: the same caps and dash ratios as `pdf::dash`.
fn stroke(style: LineStyle, lw: f32) -> Option<Stroke> {
    let (line_cap, dash) = match style {
        LineStyle::None => return None,
        LineStyle::Solid => (LineCap::Butt, None),
        LineStyle::Dashed => (LineCap::Butt, StrokeDash::new(vec![lw * 6.0, lw * 4.0], 0.0)),
        // A (near) zero-length dash with a round cap is a dot.
        LineStyle::Dotted => (LineCap::Round, StrokeDash::new(vec![lw * 0.01, lw * 2.99], 0.0)),
    };
    Some(Stroke { width: lw, line_cap, dash, ..Default::default() })
}

fn guides(band: &mut RgbaImage, t: Transform, doc: &Document, cx: f32, cy: f32, hair: f32) {
    let g = &doc.sheet.guides;
    let gray = paint([(g.gray * 255.0) as u8; 3]);
    let lw = (g.weight_pt / PT_PER_MM).max(hair);
    let cut_r = doc.sheet.cut_mm() / 2.0;
    let mut c = canvas(band);
    if let (Some(s), Some(circle)) = (stroke(g.cut, lw), PathBuilder::from_circle(cx, cy, cut_r)) {
        c.stroke_path(&circle, &gray, &s, t, None);
    }
    if g.crop_marks {
        let (a, b) = (cut_r + CROP_TICK_START, cut_r + CROP_TICK_END);
        let mut ticks = PathBuilder::new();
        for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
            ticks.move_to(cx + dx * a, cy + dy * a);
            ticks.line_to(cx + dx * b, cy + dy * b);
        }
        if let (Some(ticks), Some(s)) = (ticks.finish(), stroke(LineStyle::Solid, lw)) {
            c.stroke_path(&ticks, &gray, &s, t, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Guides, Sheet};

    fn page(doc: &Document, dpi: (f32, f32)) -> RgbaImage {
        render_band(doc, 0, dpi, 0, page_px(doc, dpi).1, &mut Badges::new())
    }

    /// Printed extent in mm of the dark pixels along a row (`across`) or column through the centre of a cell
    /// in the middle of the page, ignoring the neighbouring circles.
    fn circle_mm(doc: &Document, dpi: (f32, f32), across: bool) -> f32 {
        let page = page(doc, dpi);
        let (w_mm, h_mm) = doc.sheet.page_mm();
        let cells = doc.sheet.cells();
        let (cx, cy) = cells[cells.len() / 2];
        let (kx, ky) = (dpi.0 / INCH * doc.calibration.0, dpi.1 / INCH * doc.calibration.1);
        let (px, py) = (kx * cx + w_mm / 2.0 * (dpi.0 / INCH - kx), ky * cy + h_mm / 2.0 * (dpi.1 / INCH - ky));
        let (centre, k, len) = if across { (px, kx, page.width()) } else { (py, ky, page.height()) };
        let dark: Vec<u32> = (0..len)
            .filter(|&i| {
                let p = if across { page.get_pixel(i, py as u32) } else { page.get_pixel(px as u32, i) };
                p[0] < 200 && (i as f32 - centre).abs() < doc.sheet.cut_mm() * k * 0.52
            })
            .collect();
        (dark.last().unwrap() - dark[0]) as f32 / (if across { dpi.0 } else { dpi.1 } / INCH)
    }

    #[test]
    fn cut_circle_keeps_its_size_on_both_axes() {
        let sheet = Sheet { guides: Guides { cut: LineStyle::Solid, crop_marks: false, ..Default::default() }, ..Sheet::default() };
        let pages = vec![vec![None; sheet.cells().len()]];
        let doc = |calibration| Document { sheet: &sheet, images: &[], pages: Pages::Badges(&pages), calibration, title: "t" };
        let cut = sheet.cut_mm();
        // Square pixels, no calibration.
        let plain = doc((1.0, 1.0));
        for across in [true, false] {
            let mm = circle_mm(&plain, (100.0, 100.0), across);
            assert!((mm - cut).abs() < 1.0, "plain: measured {mm} vs {cut}");
        }
        // A printer with twice the resolution down the page, and a 10% correction on that axis only.
        let skewed = doc((1.0, 1.1));
        let (wide, tall) = (circle_mm(&skewed, (100.0, 200.0), true), circle_mm(&skewed, (100.0, 200.0), false));
        assert!((wide - cut).abs() < 1.0, "skewed: {wide} mm wide vs {cut}");
        assert!((tall - cut * 1.1).abs() < 1.0, "skewed: {tall} mm tall vs {}", cut * 1.1);
    }

    #[test]
    fn bands_stitch_into_the_same_page() {
        // Badges, dotted circles, crop marks and coloured discs, cut into bands that split them mid-way.
        let sheet = Sheet { background: Some([0.9, 0.6, 0.2]), guides: Guides { cut: LineStyle::Dotted, crop_marks: true, ..Default::default() }, ..Sheet::default() };
        let images = [crate::pdf::tests::sample()];
        let pages = crate::layout::paginate(&sheet.cells(), &[4]);
        let doc = Document { sheet: &sheet, images: &images, pages: Pages::Badges(&pages), calibration: (1.01, 0.99), title: "t" };
        let dpi = (150.0, 150.0);
        let whole = page(&doc, dpi);
        let (mut stitched, mut badges) = (Vec::new(), Badges::new());
        for y0 in (0..whole.height()).step_by(97) {
            stitched.extend(render_band(&doc, 0, dpi, y0, 97.min(whole.height() - y0), &mut badges).into_raw());
        }
        assert_eq!(stitched.len(), whole.as_raw().len());
        // tiny-skia flattens a curve that crosses the band edge slightly differently, so edge pixels may differ
        // by a fraction of their coverage. A band drawn one row out would differ by far more than this.
        let worst = stitched.iter().zip(whole.as_raw()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 64, "a band differs from the whole page by {worst}/255");
        assert!(whole.pixels().any(|p| p[0] != 255), "nothing was drawn");
    }
}
