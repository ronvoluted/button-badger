//! Bitmap page renderer (used for Windows printing). Same geometry as the PDF path: everything is drawn in mm
//! through one mm → pixel transform, in horizontal bands so a page is never held in memory whole.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::images::compose;
use crate::layout::{CROP_TICK_END, CROP_TICK_START, INCH, LineStyle, PT_PER_MM, TestPage};
use crate::pdf::{Document, Pages};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use resvg::tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, PixmapMut, Stroke, StrokeDash, Transform};
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
                } else if let (Some(bg), Some(disc)) = (sheet.background, circle(cx, cy, cut_r, hair)) {
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
                if let (Some(circle), Some(s)) = (circle(bx, by, d / 2.0, hair), stroke(LineStyle::Solid, 0.2f32.max(hair))) {
                    c.stroke_path(&circle, &black, &s, t, None);
                }
            }
        }
    }
    band
}

/// A circle as a polygon whose sides stray less than 1% of a pixel (`hair` mm) from the arc. tiny-skia flattens
/// curves far more coarsely than that, and differently when a band edge cuts through one, which left seams
/// between bands. Straight sides come out the same in every band.
fn circle(cx: f32, cy: f32, r: f32, hair: f32) -> Option<Path> {
    use std::f32::consts::TAU;
    // A side spanning angle `a` bows r·a²/8 away from the arc: 0.07 keeps that just under 1% of `hair`.
    let sides = (TAU / (0.07 * hair / r).sqrt()).ceil().clamp(8.0, 8192.0);
    let mut path = PathBuilder::new();
    path.move_to(cx + r, cy);
    for i in 1..sides as u32 {
        let (sin, cos) = (i as f32 * TAU / sides).sin_cos();
        path.line_to(cx + r * cos, cy + r * sin);
    }
    path.close();
    path.finish()
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
    if let (Some(s), Some(circle)) = (stroke(g.cut, lw), circle(cx, cy, cut_r, hair)) {
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

    /// Ink (0 = bare paper) in each pixel of row `at` of the page (`across`), or of column `at`.
    fn scan(doc: &Document, dpi: (f32, f32), across: bool, at: u32) -> Vec<f64> {
        let ink = |p: &Rgba<u8>| 255.0 - p[0] as f64;
        if across {
            render_band(doc, 0, dpi, at, 1, &mut Badges::new()).pixels().map(ink).collect()
        } else {
            let page = page(doc, dpi);
            (0..page.height()).map(|y| ink(page.get_pixel(at, y))).collect()
        }
    }

    /// Middle of the ink of a line crossing the scan within `reach` pixels of `near`, to a fraction of a pixel.
    fn line_centre(scan: &[f64], near: f64, reach: f64) -> f64 {
        let span = (near - reach) as usize..(near + reach) as usize;
        let ink: f64 = scan[span.clone()].iter().sum();
        span.map(|i| (i as f64 + 0.5) * scan[i]).sum::<f64>() / ink
    }

    /// Page pixel of a point `mm` along an axis `page_mm` long: calibration scales about the middle of the page.
    fn px(mm: f32, page_mm: f32, calibration: f32, dpi: f32) -> f64 {
        let (mm, half, cal, dpi) = (mm as f64, page_mm as f64 / 2.0, calibration as f64, dpi as f64);
        (half + (mm - half) * cal) * dpi / INCH as f64
    }

    #[test]
    fn cut_circle_keeps_its_size_and_place_on_both_axes() {
        let sheet = Sheet { guides: Guides { cut: LineStyle::Solid, crop_marks: false, ..Default::default() }, ..Sheet::default() };
        let cells = sheet.cells();
        let pages = vec![vec![None; cells.len()]];
        let (page_mm, centre_mm, cut) = (sheet.page_mm(), cells[cells.len() / 2], sheet.cut_mm());
        // Square pixels and no correction, then a printer with twice the resolution down the page and a 10%
        // correction on that axis only.
        for (dpi, calibration) in [((600.0, 600.0), (1.0, 1.0)), ((600.0, 1200.0), (1.0, 1.1))] {
            let doc = Document { sheet: &sheet, images: &[], pages: Pages::Badges(&pages), calibration, title: "t" };
            let centre = (px(centre_mm.0, page_mm.0, calibration.0, dpi.0), px(centre_mm.1, page_mm.1, calibration.1, dpi.1));
            for across in [true, false] {
                let (c, at, dpi_along, cal) = if across { (centre.0, centre.1, dpi.0, calibration.0) } else { (centre.1, centre.0, dpi.1, calibration.1) };
                let per_mm = (dpi_along / INCH) as f64;
                let scan = scan(&doc, dpi, across, at as u32);
                // Each side of the circle is looked for within 0.5 mm of where it belongs; the next circle is 3 mm away.
                let r = (cut * cal) as f64 / 2.0 * per_mm;
                let (a, b) = (line_centre(&scan, c - r, 0.5 * per_mm), line_centre(&scan, c + r, 0.5 * per_mm));
                let (size, place) = ((b - a - 2.0 * r) / per_mm, ((a + b) / 2.0 - c) / per_mm);
                assert!(size.abs() < 0.01 && place.abs() < 0.01, "{dpi:?} across={across}: size off by {size} mm, place by {place} mm");
            }
        }
        // Those scans cross the circle at corners of the polygon it is drawn as. Between corners, its sides stay
        // within 1% of a pixel of the true circle.
        let (r, hair) = (cut / 2.0, INCH / 600.0);
        let corners = circle(0.0, 0.0, r, hair).unwrap().points().to_vec();
        let bow = corners.windows(2).map(|side| r - ((side[0].x + side[1].x) / 2.0).hypot((side[0].y + side[1].y) / 2.0)).fold(0.0, f32::max);
        assert!(bow < hair * 0.01, "the polygon's sides bow {bow} mm inside the circle");
    }

    #[test]
    fn calibration_holds_to_four_decimal_places() {
        // The app reports its correction to four places, i.e. 0.01 mm over the 100 mm rules of the test page.
        let sheet = Sheet::default();
        let calibration = (1.0012, 0.9988);
        let doc = Document { sheet: &sheet, images: &[], pages: Pages::TestPage, calibration, title: "t" };
        let (page_mm, test) = (sheet.page_mm(), TestPage::new(&sheet));
        // Fine pixels along the rule being measured, coarse ones the other way to keep the bitmap small.
        let (fine, coarse) = (2400.0, 72.0);
        let per_mm = (fine / INCH) as f64;
        // Scanning 3 mm to one side of a rule crosses only its long ticks, which mark its two ends (and middle).
        let across = scan(&doc, (fine, coarse), true, px(test.across.1 - 3.0, page_mm.1, calibration.1, coarse) as u32);
        let down = scan(&doc, (coarse, fine), false, px(test.down.0 - 3.0, page_mm.0, calibration.0, coarse) as u32);
        for (scan, start, page_mm, cal) in [(across, test.across.0, page_mm.0, calibration.0), (down, test.down.1, page_mm.1, calibration.1)] {
            let end = |mm: f32| line_centre(&scan, px(mm, page_mm, cal, fine), per_mm);
            let printed = (end(start + test.rule) - end(start)) / per_mm;
            assert!((printed - (test.rule * cal) as f64).abs() < 0.005, "the {} mm rule at ×{cal} printed {printed} mm long", test.rule);
        }
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
        assert!(whole.pixels().any(|p| p[0] != 255), "nothing was drawn");
        // tiny-skia anti-aliases with 16 samples per pixel, and a band's offset can round a line across one of
        // them. So an edge pixel may differ by a sample or two of its coverage (16/255 each), and very few do.
        // Curves flattened differently per band differed by 55, and a band drawn one row out differs by 255.
        let diffs = stitched.iter().zip(whole.as_raw()).map(|(a, b)| a.abs_diff(*b));
        let (worst, count) = diffs.fold((0, 0), |(worst, count), d| (worst.max(d), count + usize::from(d > 0)));
        assert!(worst <= 32, "a band differs from the whole page by {worst}/255");
        assert!(count * 2000 < stitched.len(), "{count} of {} values differ between bands and the whole page", stitched.len());
    }
}
