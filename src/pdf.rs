//! Vector PDF output: one shared image XObject per source image, circle clips, dashed guides.

use crate::images::Loaded;
use crate::layout::{CROP_TICK_END, CROP_TICK_START, INCH, LineStyle, PT_PER_MM, Sheet, TestPage, trim_float};
use pdf_writer::types::LineCapStyle;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Max resolution embedded for an image, in dots per inch at its printed size.
const EMBED_DPI: f32 = 600.0;

/// What a document prints: badge pages (image index per cell, per page) or the calibration page.
#[derive(Clone, Copy)]
pub enum Pages<'a> {
    Badges(&'a [Vec<Option<usize>>]),
    TestPage,
}

/// Everything needed to render a print job, shared by the PDF and raster (Windows) backends.
pub struct Document<'a> {
    pub sheet: &'a Sheet,
    pub images: &'a [Loaded],
    pub pages: Pages<'a>,
    /// Printer scale correction (x, y), applied about the page centre.
    pub calibration: (f32, f32),
    pub title: &'a str,
}

impl Document<'_> {
    pub fn page_count(&self) -> usize {
        match self.pages {
            Pages::Badges(pages) => pages.len().max(1),
            Pages::TestPage => 1,
        }
    }

    pub fn pdf(&self) -> Vec<u8> {
        let mut pdf = Pdf::new();
        let mut next = 0;
        let mut alloc = || {
            next += 1;
            Ref::new(next)
        };
        let catalog = alloc();
        let pages_id = alloc();
        let font_id = alloc();
        pdf.catalog(catalog).pages(pages_id);
        pdf.type1_font(font_id).base_font(Name(b"Helvetica")).encoding_predefined(Name(b"WinAnsiEncoding"));

        // One image XObject per used image, referenced by every cell that shows it.
        let mut xobjects: BTreeMap<usize, Ref> = BTreeMap::new();
        if let Pages::Badges(pages) = self.pages {
            for &i in pages.iter().flatten().flatten() {
                if let (None, Some(img)) = (xobjects.get(&i), self.images.get(i)) {
                    let (id, mask) = (alloc(), alloc());
                    embed(&mut pdf, id, mask, self.sheet, img);
                    xobjects.insert(i, id);
                }
            }
        }

        let page_ids: Vec<Ref> = (0..self.page_count()).map(|_| alloc()).collect();
        pdf.pages(pages_id).kids(page_ids.iter().copied()).count(page_ids.len() as i32);

        let (w_mm, h_mm) = self.sheet.page_mm();
        let (w_pt, h_pt) = (w_mm * PT_PER_MM, h_mm * PT_PER_MM);
        for (n, &page_id) in page_ids.iter().enumerate() {
            let mut c = Content::new();
            // Draw in mm with y down; calibration scales about the page centre.
            let (sx, sy) = self.calibration;
            c.transform([sx * PT_PER_MM, 0.0, 0.0, -sy * PT_PER_MM, w_pt * (1.0 - sx) / 2.0, h_pt * (1.0 + sy) / 2.0]);
            match self.pages {
                Pages::Badges(pages) => self.badge_page(&mut c, pages.get(n).map_or(&[], Vec::as_slice)),
                Pages::TestPage => self.test_page(&mut c),
            }
            let content_id = alloc();
            pdf.stream(content_id, &c.finish());
            let mut page = pdf.page(page_id);
            page.parent(pages_id).media_box(Rect::new(0.0, 0.0, w_pt, h_pt)).contents(content_id);
            let mut res = page.resources();
            res.fonts().pair(Name(b"F1"), font_id);
            let mut xo = res.x_objects();
            for (&i, &id) in &xobjects {
                xo.pair(Name(xobject_name(i).as_bytes()), id);
            }
            xo.finish();
            res.finish();
            page.finish();
        }
        pdf.finish()
    }

    fn badge_page(&self, c: &mut Content, items: &[Option<usize>]) {
        let sheet = self.sheet;
        let cut_r = sheet.cut_mm() / 2.0;
        for (n, (cx, cy)) in sheet.cells().into_iter().enumerate() {
            if let Some(bg) = sheet.background {
                c.set_fill_rgb(bg[0], bg[1], bg[2]);
                circle(c, cx, cy, cut_r);
                c.fill_nonzero();
            }
            let item = items.get(n).copied().flatten();
            if let Some((i, img)) = item.and_then(|i| self.images.get(i).map(|img| (i, img))) {
                let r = sheet.image_rect(img.width(), img.height(), img.zoom, img.offset_mm);
                c.save_state();
                if sheet.guides.clip {
                    circle(c, cx, cy, cut_r);
                    c.clip_nonzero();
                    c.end_path();
                }
                // Rotate about the cell centre, then map the image's unit square (y up) into y-down mm.
                let (sin, cos) = img.rotation.to_radians().sin_cos();
                c.transform([cos, sin, -sin, cos, cx, cy]);
                c.transform([r.w, 0.0, 0.0, -r.h, r.x, r.y + r.h]);
                c.x_object(Name(xobject_name(i).as_bytes()));
                c.restore_state();
            }
            guides(c, sheet, cx, cy);
        }
    }

    fn test_page(&self, c: &mut Content) {
        let sheet = self.sheet;
        let (w, _) = sheet.page_mm();
        let t = TestPage::new(sheet);
        let (cut, face) = (sheet.cut_mm(), sheet.face_mm);
        c.set_stroke_gray(0.0);
        c.set_line_width(0.25);
        for [(x1, y1), (x2, y2)] in t.segments() {
            c.move_to(x1, y1).line_to(x2, y2);
        }
        c.stroke();
        // Both circles solid and thin so they can be measured, whatever the guide style.
        let (bx, by) = t.badge;
        c.set_line_width(0.2);
        circle(c, bx, by, cut / 2.0);
        c.stroke();
        circle(c, bx, by, face / 2.0);
        c.stroke();

        let m = TestPage::MARGIN;
        let rule = format!("{} mm", trim_float(t.rule, 0));
        let subtitle = if w >= 180.0 {
            format!("Print at 100% (no scaling), measure both {rule} rules, then enter the lengths in the app.")
        } else {
            format!("Print at 100%. Measure both {rule} rules.")
        };
        let lines = [
            (w / 2.0, m + 5.0, 5.0, "Button Badger · printer calibration".to_string()),
            (w / 2.0, m + 12.0, 3.0, subtitle),
            (t.across.0 + t.rule / 2.0, t.across.1 + 9.0, 3.2, format!("{rule} across")),
            (t.down.0, t.down.1 + t.rule + 7.0, 3.2, format!("{rule} down")),
            (bx, by + cut / 2.0 + 8.0, 3.2, format!("cut Ø {} mm · face Ø {} mm", trim_float(cut, 1), trim_float(face, 1))),
        ];
        c.set_fill_gray(0.0);
        for (cx, y, size, text) in lines {
            let bytes = win_ansi(&text);
            // Centre using Helvetica's average advance (~0.5 em).
            let x = cx - bytes.len() as f32 * size * 0.25;
            c.begin_text();
            c.set_font(Name(b"F1"), size);
            c.set_text_matrix([1.0, 0.0, 0.0, -1.0, x, y]);
            c.show(Str(&bytes));
            c.end_text();
        }
    }
}

fn xobject_name(image: usize) -> String {
    format!("Im{image}")
}

/// Encode text for the built-in Helvetica font (WinAnsi); unknown characters become '?'.
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|ch| match ch {
            '·' => 0xB7,
            'Ø' => 0xD8,
            '×' => 0xD7,
            '°' => 0xB0,
            '“' => 0x93,
            '”' => 0x94,
            c if c.is_ascii() => c as u8,
            _ => b'?',
        })
        .collect()
}

fn embed(pdf: &mut Pdf, id: Ref, mask: Ref, sheet: &Sheet, img: &Loaded) {
    let r = sheet.image_rect(img.width(), img.height(), img.zoom, img.offset_mm);
    let max_w = ((r.w / INCH) * EMBED_DPI).ceil() as u32;
    let src = if img.width() > max_w {
        let s = max_w as f32 / img.width() as f32;
        let h = ((img.height() as f32 * s).round() as u32).max(1);
        Cow::Owned(image::imageops::resize(&*img.full, max_w.max(1), h, image::imageops::FilterType::Lanczos3))
    } else {
        Cow::Borrowed(&*img.full)
    };
    let (w, h) = src.dimensions();
    let rgb: Vec<u8> = src.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let rgb_z = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6);
    let mut x = pdf.image_xobject(id, &rgb_z);
    x.filter(Filter::FlateDecode);
    x.width(w as i32).height(h as i32).bits_per_component(8).interpolate(true);
    x.color_space().device_rgb();
    if img.has_alpha {
        x.s_mask(mask);
    }
    x.finish();
    if img.has_alpha {
        let a: Vec<u8> = src.pixels().map(|p| p[3]).collect();
        let a_z = miniz_oxide::deflate::compress_to_vec_zlib(&a, 6);
        let mut m = pdf.image_xobject(mask, &a_z);
        m.filter(Filter::FlateDecode);
        m.width(w as i32).height(h as i32).bits_per_component(8);
        m.color_space().device_gray();
        m.finish();
    }
}

fn circle(c: &mut Content, cx: f32, cy: f32, r: f32) {
    let k = 0.5522848 * r;
    c.move_to(cx + r, cy);
    c.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    c.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    c.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    c.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    c.close_path();
}

fn guides(c: &mut Content, sheet: &Sheet, cx: f32, cy: f32) {
    let g = &sheet.guides;
    let lw = g.weight_pt / PT_PER_MM;
    let cut_r = sheet.cut_mm() / 2.0;
    c.save_state();
    c.set_stroke_gray(g.gray);
    c.set_line_width(lw);
    match g.cut {
        LineStyle::None => {}
        style => {
            dash(c, style, lw);
            circle(c, cx, cy, cut_r);
            c.stroke();
        }
    }
    if g.crop_marks {
        dash(c, LineStyle::Solid, lw);
        let (a, b) = (cut_r + CROP_TICK_START, cut_r + CROP_TICK_END);
        c.move_to(cx, cy - a).line_to(cx, cy - b);
        c.move_to(cx, cy + a).line_to(cx, cy + b);
        c.move_to(cx - a, cy).line_to(cx - b, cy);
        c.move_to(cx + a, cy).line_to(cx + b, cy);
        c.stroke();
    }
    c.restore_state();
}

fn dash(c: &mut Content, style: LineStyle, lw: f32) {
    match style {
        LineStyle::Dotted => {
            c.set_line_cap(LineCapStyle::RoundCap);
            c.set_dash_pattern([0.0, lw * 3.0], 0.0);
        }
        LineStyle::Dashed => {
            c.set_line_cap(LineCapStyle::ButtCap);
            c.set_dash_pattern([lw * 6.0, lw * 4.0], 0.0);
        }
        LineStyle::Solid | LineStyle::None => {
            c.set_line_cap(LineCapStyle::ButtCap);
            c.set_dash_pattern([], 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Paper;
    use std::sync::Arc;

    fn sample() -> Loaded {
        let mut img = image::RgbaImage::new(400, 300);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let a = if (x / 50 + y / 50) % 2 == 0 { 255 } else { 0 };
            *p = image::Rgba([220, 90, 50, a]);
        }
        Loaded {
            path: "sample.png".into(),
            name: "sample".into(),
            full: Arc::new(img.clone()),
            preview: Arc::new(img),
            has_alpha: true,
            qty: 0,
            zoom: 1.0,
            offset_mm: (0.0, 0.0),
            rotation: 0.0,
        }
    }

    #[test]
    fn writes_valid_pdf_with_shared_image() {
        let sheet = Sheet { background: Some([0.95, 0.9, 0.8]), ..Sheet::default() };
        let images = [sample()];
        let pages = crate::layout::paginate(&sheet.cells(), &[0]);
        let doc = Document { sheet: &sheet, images: &images, pages: Pages::Badges(&pages), calibration: (1.01, 0.99), title: "t" };
        let bytes = doc.pdf();
        assert!(bytes.starts_with(b"%PDF-"));
        // One image + one soft mask, regardless of how many cells use it.
        assert_eq!(String::from_utf8_lossy(&bytes).matches("/Subtype /Image").count(), 2);

        for paper in [Paper::A4, Paper::A6] {
            let sheet = Sheet { paper, ..Sheet::default() };
            let test = Document { sheet: &sheet, images: &[], pages: Pages::TestPage, calibration: (1.0, 1.0), title: "t" };
            assert!(test.pdf().starts_with(b"%PDF-"));
        }
    }
}
