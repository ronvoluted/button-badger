//! Pure geometry: papers, badge presets, cell placement, pagination. All lengths in mm.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const INCH: f32 = 25.4;
pub const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Paper {
    A4,
    A5,
    A6,
    Letter,
    Legal,
    Tabloid,
    Custom,
}

impl Paper {
    pub const ALL: [Paper; 7] = [
        Paper::A4,
        Paper::A5,
        Paper::A6,
        Paper::Letter,
        Paper::Legal,
        Paper::Tabloid,
        Paper::Custom,
    ];

    /// Portrait size in mm. `custom` is used for `Paper::Custom`.
    pub fn size_mm(self, custom: (f32, f32)) -> (f32, f32) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::A5 => (148.0, 210.0),
            Paper::A6 => (105.0, 148.0),
            Paper::Letter => (8.5 * INCH, 11.0 * INCH),
            Paper::Legal => (8.5 * INCH, 14.0 * INCH),
            Paper::Tabloid => (11.0 * INCH, 17.0 * INCH),
            Paper::Custom => custom,
        }
    }
}

impl fmt::Display for Paper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Paper::A4 => "A4",
            Paper::A5 => "A5",
            Paper::A6 => "A6",
            Paper::Letter => "US Letter",
            Paper::Legal => "US Legal",
            Paper::Tabloid => "Tabloid",
            Paper::Custom => "Custom…",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Packing {
    Grid,
    Hex,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Units {
    Mm,
    Inch,
}

impl Units {
    pub fn to_mm(self, v: f32) -> f32 {
        match self {
            Units::Mm => v,
            Units::Inch => v * INCH,
        }
    }
    pub fn suffix(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Inch => "in",
        }
    }
    pub fn format(self, mm: f32) -> String {
        match self {
            Units::Mm => trim_float(mm, 1),
            Units::Inch => trim_float(mm / INCH, 3),
        }
    }
}

pub fn trim_float(v: f32, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineStyle {
    Dotted,
    Dashed,
    Solid,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    /// Longest image side equals the face diameter (whole image visible on the face).
    Fit,
    /// Shortest image side covers the cut diameter (image bleeds to the cut line).
    Fill,
}

/// Badge presets: visible face diameter and the bleed each side (wrap-around material).
pub struct Preset {
    pub name: &'static str,
    pub face_mm: f32,
    pub bleed_mm: f32,
}

pub const PRESETS: &[Preset] = &[
    Preset { name: "25 mm · 1\"", face_mm: 25.0, bleed_mm: 3.0 },
    Preset { name: "32 mm · 1¼\"", face_mm: 32.0, bleed_mm: 2.5 },
    Preset { name: "37 mm · 1½\"", face_mm: 37.0, bleed_mm: 3.0 },
    Preset { name: "44 mm · 1¾\"", face_mm: 44.0, bleed_mm: 3.0 },
    Preset { name: "56 mm · 2¼\"", face_mm: 56.0, bleed_mm: 6.0 },
    Preset { name: "58 mm", face_mm: 58.0, bleed_mm: 6.0 },
    Preset { name: "75 mm · 3\"", face_mm: 75.0, bleed_mm: 6.0 },
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Guides {
    pub cut: LineStyle,
    /// Framing aid: the face circle, drawn in the preview only.
    pub face: bool,
    pub crop_marks: bool,
    pub clip: bool,
    /// Line weight in points.
    pub weight_pt: f32,
    /// Grey level 0 (black) – 1 (white).
    pub gray: f32,
}

impl Default for Guides {
    fn default() -> Self {
        Guides {
            cut: LineStyle::Dotted,
            face: false,
            crop_marks: false,
            clip: true,
            weight_pt: 0.4,
            gray: 0.45,
        }
    }
}

// `serde(default)` so settings written by an older version (missing fields) still load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sheet {
    pub paper: Paper,
    pub custom_paper_mm: (f32, f32),
    pub orientation: Orientation,
    pub margin_mm: f32,
    pub gap_mm: f32,
    pub packing: Packing,
    pub face_mm: f32,
    pub bleed_mm: f32,
    pub guides: Guides,
    pub units: Units,
    pub fit: Fit,
    /// Fill behind the image (useful for transparent art). `None` leaves paper white.
    pub background: Option<[f32; 3]>,
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet {
            paper: Paper::A4,
            custom_paper_mm: (200.0, 200.0),
            orientation: Orientation::Portrait,
            margin_mm: 6.0,
            gap_mm: 3.0,
            packing: Packing::Auto,
            face_mm: 32.0,
            bleed_mm: 2.5,
            guides: Guides::default(),
            units: Units::Mm,
            fit: Fit::Fill,
            background: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Sheet {
    pub fn cut_mm(&self) -> f32 {
        self.face_mm + 2.0 * self.bleed_mm
    }

    /// Oriented page size (w, h) in mm.
    pub fn page_mm(&self) -> (f32, f32) {
        let (w, h) = self.paper.size_mm(self.custom_paper_mm);
        match self.orientation {
            Orientation::Portrait => (w.min(h), w.max(h)),
            Orientation::Landscape => (w.max(h), w.min(h)),
        }
    }

    pub fn printable(&self) -> Rect {
        let (w, h) = self.page_mm();
        let m = self.margin_mm.max(0.0);
        Rect { x: m, y: m, w: (w - 2.0 * m).max(0.0), h: (h - 2.0 * m).max(0.0) }
    }

    /// Centres of every badge on one page, in mm from the page's top-left corner.
    pub fn cells(&self) -> Vec<(f32, f32)> {
        let area = self.printable();
        let cut = self.cut_mm();
        let pitch = cut + self.gap_mm.max(0.0);
        match self.packing {
            Packing::Grid => grid_cells(area, cut, pitch),
            Packing::Hex => hex_cells(area, cut, pitch),
            Packing::Auto => {
                let g = grid_cells(area, cut, pitch);
                let h = hex_cells(area, cut, pitch);
                if h.len() > g.len() { h } else { g }
            }
        }
    }

    /// Rect (relative to the cell centre, mm) that image `w × h` px occupies.
    pub fn image_rect(&self, w: u32, h: u32, zoom: f32, offset_mm: (f32, f32)) -> Rect {
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let base = match self.fit {
            Fit::Fit => self.face_mm / w.max(h),
            Fit::Fill => self.cut_mm() / w.min(h),
        };
        let s = base * zoom.max(0.01);
        let (rw, rh) = (w * s, h * s);
        Rect { x: offset_mm.0 - rw / 2.0, y: offset_mm.1 - rh / 2.0, w: rw, h: rh }
    }

    /// Effective print resolution of an image placed on this badge.
    pub fn effective_dpi(&self, w: u32, h: u32, zoom: f32) -> f32 {
        let r = self.image_rect(w, h, zoom, (0.0, 0.0));
        w as f32 / (r.w / INCH)
    }
}

/// Crop ticks run from this far outside the cut circle (mm)…
pub const CROP_TICK_START: f32 = 1.0;
/// …to this far.
pub const CROP_TICK_END: f32 = 3.0;

/// Calibration test page geometry in mm, shared by the PDF and raster renderers.
pub struct TestPage {
    /// Length of both rules: 100 mm when it fits, otherwise a shorter round figure.
    pub rule: f32,
    /// Left end of the horizontal rule.
    pub across: (f32, f32),
    /// Top end of the vertical rule.
    pub down: (f32, f32),
    /// Centre of the sample badge drawn beside the vertical rule.
    pub badge: (f32, f32),
}

impl TestPage {
    pub const MARGIN: f32 = 12.0;

    pub fn new(sheet: &Sheet) -> TestPage {
        let (w, h) = sheet.page_mm();
        let m = Self::MARGIN;
        // Below the title: 24 mm to the across rule, 20 more to the down rule, ~10 for its label.
        let room = (h - m - 24.0 - 20.0 - m - 10.0).min(w - 2.0 * m);
        let rule = ((room / 10.0).floor() * 10.0).clamp(30.0, 100.0);
        let across = ((w - rule) / 2.0, m + 24.0);
        // The down rule (8 mm wide with ticks) and the badge are centred together, 24 mm apart.
        let cut = sheet.cut_mm();
        let left = (w - (8.0 + 24.0 + cut)) / 2.0;
        let down = (left + 4.0, across.1 + 20.0);
        let badge = (left + 8.0 + 24.0 + cut / 2.0, down.1 + rule / 2.0);
        TestPage { rule, across, down, badge }
    }

    /// Both rules as line segments, with a tick every 10 mm (longer every 50 mm).
    pub fn segments(&self) -> Vec<[(f32, f32); 2]> {
        let ((ax, ay), (dx, dy)) = (self.across, self.down);
        let mut out = vec![[(ax, ay), (ax + self.rule, ay)], [(dx, dy), (dx, dy + self.rule)]];
        for i in 0..=(self.rule / 10.0) as i32 {
            let (o, t) = (i as f32 * 10.0, if i % 5 == 0 { 4.0 } else { 2.0 });
            out.push([(ax + o, ay - t), (ax + o, ay + t)]);
            out.push([(dx - t, dy + o), (dx + t, dy + o)]);
        }
        out
    }
}

fn grid_cells(area: Rect, cut: f32, pitch: f32) -> Vec<(f32, f32)> {
    if cut <= 0.0 || area.w < cut || area.h < cut {
        return Vec::new();
    }
    let cols = ((area.w - cut) / pitch).floor() as usize + 1;
    let rows = ((area.h - cut) / pitch).floor() as usize + 1;
    let used_w = cut + (cols - 1) as f32 * pitch;
    let used_h = cut + (rows - 1) as f32 * pitch;
    let x0 = area.x + (area.w - used_w) / 2.0 + cut / 2.0;
    let y0 = area.y + (area.h - used_h) / 2.0 + cut / 2.0;
    let mut out = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            out.push((x0 + c as f32 * pitch, y0 + r as f32 * pitch));
        }
    }
    out
}

fn hex_cells(area: Rect, cut: f32, pitch: f32) -> Vec<(f32, f32)> {
    if cut <= 0.0 || area.w < cut || area.h < cut {
        return Vec::new();
    }
    let row_step = pitch * 3f32.sqrt() / 2.0;
    let rows = ((area.h - cut) / row_step).floor() as usize + 1;
    let cols_even = ((area.w - cut) / pitch).floor() as usize + 1;
    // Odd rows shift by half a pitch and lose a column if the shifted row would overflow.
    let cols_odd = if cut + (cols_even - 1) as f32 * pitch + pitch / 2.0 <= area.w { cols_even } else { cols_even - 1 };
    let used_w = cut + (cols_even - 1) as f32 * pitch + if rows > 1 && cols_odd == cols_even { pitch / 2.0 } else { 0.0 };
    let used_h = cut + (rows - 1) as f32 * row_step;
    let x0 = area.x + (area.w - used_w) / 2.0 + cut / 2.0;
    let y0 = area.y + (area.h - used_h) / 2.0 + cut / 2.0;
    let mut out = Vec::with_capacity(rows * cols_even);
    for r in 0..rows {
        let odd = r % 2 == 1;
        let cols = if odd { cols_odd } else { cols_even };
        let shift = if odd { pitch / 2.0 } else { 0.0 };
        for c in 0..cols {
            out.push((x0 + shift + c as f32 * pitch, y0 + r as f32 * row_step));
        }
    }
    out
}

/// Distribute images over pages. `qty[i]` copies of image `i`; `0` means "fill remaining cells".
/// Returns, per page, the image index for each cell (None = empty cell). Always at least one page.
pub fn paginate(cells_per_page: usize, qty: &[u32]) -> Vec<Vec<Option<usize>>> {
    if cells_per_page == 0 {
        return vec![Vec::new()];
    }
    let mut seq: Vec<Option<usize>> = Vec::new();
    for (i, &q) in qty.iter().enumerate() {
        seq.extend(std::iter::repeat_n(Some(i), q as usize));
    }
    let fillers: Vec<usize> = qty.iter().enumerate().filter(|(_, q)| **q == 0).map(|(i, _)| i).collect();
    let pages = seq.len().div_ceil(cells_per_page).max(1);
    let total = pages * cells_per_page;
    let mut k = 0;
    while seq.len() < total {
        seq.push(if fillers.is_empty() { None } else { Some(fillers[k % fillers.len()]) });
        k += 1;
    }
    seq.chunks(cells_per_page).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a4(face: f32, bleed: f32, gap: f32, margin: f32) -> Sheet {
        Sheet { face_mm: face, bleed_mm: bleed, gap_mm: gap, margin_mm: margin, packing: Packing::Grid, fit: Fit::Fit, ..Sheet::default() }
    }

    #[test]
    fn grid_counts() {
        // 37mm cut, 3mm gap, 5mm margin on A4: 200/40 → 5 cols, 287/40 → 7 rows.
        let s = a4(32.0, 2.5, 3.0, 5.0);
        assert_eq!(s.cells().len(), 35);
        let s = Sheet { orientation: Orientation::Landscape, ..s };
        assert_eq!(s.cells().len(), 7 * 4 + 7); // 7 cols × 5 rows
        let big = a4(300.0, 0.0, 0.0, 0.0);
        assert!(big.cells().is_empty());
    }

    #[test]
    fn hex_fits_at_least_as_many_in_auto() {
        let s = Sheet { packing: Packing::Auto, ..a4(32.0, 2.5, 3.0, 5.0) };
        let grid = Sheet { packing: Packing::Grid, ..s.clone() }.cells().len();
        assert!(s.cells().len() >= grid);
    }

    #[test]
    fn cells_stay_inside_and_never_overlap() {
        let narrow = Sheet { paper: Paper::Custom, custom_paper_mm: (50.0, 200.0), ..a4(32.0, 2.5, 3.0, 5.0) };
        for packing in [Packing::Grid, Packing::Hex, Packing::Auto] {
            for base in [a4(32.0, 2.5, 3.0, 5.0), a4(58.0, 6.0, 2.0, 6.0), narrow.clone()] {
                let s = Sheet { packing, ..base };
                let (area, r, cells) = (s.printable(), s.cut_mm() / 2.0, s.cells());
                assert!(!cells.is_empty());
                for (i, &(x, y)) in cells.iter().enumerate() {
                    assert!(x - r >= area.x - 0.01 && x + r <= area.x + area.w + 0.01, "{packing:?} x={x}");
                    assert!(y - r >= area.y - 0.01 && y + r <= area.y + area.h + 0.01, "{packing:?} y={y}");
                    for &(x2, y2) in &cells[i + 1..] {
                        assert!((x - x2).hypot(y - y2) >= 2.0 * r + s.gap_mm - 0.01, "{packing:?} overlap");
                    }
                }
            }
        }
    }

    #[test]
    fn test_page_adapts_to_paper() {
        assert_eq!(TestPage::new(&Sheet::default()).rule, 100.0);
        let a6 = Sheet { paper: Paper::A6, face_mm: 44.0, ..Sheet::default() };
        let t = TestPage::new(&a6);
        assert!(t.rule < 100.0);
        let (w, h) = a6.page_mm();
        for [(x1, y1), (x2, y2)] in t.segments() {
            assert!(x1.min(x2) >= 0.0 && x1.max(x2) <= w && y1.min(y2) >= 0.0 && y1.max(y2) <= h);
        }
    }

    #[test]
    fn pagination() {
        assert_eq!(paginate(4, &[]), vec![vec![None; 4]]);
        assert_eq!(paginate(4, &[0]), vec![vec![Some(0); 4]]);
        let p = paginate(4, &[6, 0]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1], vec![Some(0), Some(0), Some(1), Some(1)]);
        let p = paginate(3, &[2, 2]);
        assert_eq!(p, vec![vec![Some(0), Some(0), Some(1)], vec![Some(1), None, None]]);
    }

    #[test]
    fn image_fit_and_dpi() {
        let s = a4(32.0, 2.5, 3.0, 5.0);
        let r = s.image_rect(1000, 500, 1.0, (0.0, 0.0));
        assert!((r.w - 32.0).abs() < 1e-4 && (r.h - 16.0).abs() < 1e-4);
        let f = Sheet { fit: Fit::Fill, ..s.clone() }.image_rect(1000, 500, 1.0, (0.0, 0.0));
        assert!((f.h - 37.0).abs() < 1e-4);
        assert!((s.effective_dpi(1000, 1000, 1.0) - 1000.0 / (32.0 / INCH)).abs() < 0.01);
    }
}
