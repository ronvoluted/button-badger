//! Loading images (raster + SVG, which also keeps its vector tree) and compositing a single badge bitmap.

use crate::layout::Sheet;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageDecoder, ImageReader, Rgba, RgbaImage};
use resvg::usvg::fontdb::Database;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

const MAX_FULL: u32 = 4096;
const MAX_PREVIEW: u32 = 1024;
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "ico", "svg", "svgz"];

#[derive(Clone, Debug)]
pub struct Loaded {
    pub path: PathBuf,
    pub name: String,
    /// Full-resolution pixels (≤ MAX_FULL on the long edge), EXIF-oriented.
    pub full: Arc<RgbaImage>,
    /// Small copy used for the live preview.
    pub preview: Arc<RgbaImage>,
    /// The parsed SVG, embedded as vectors in the PDF; `full` is then only its bitmap for preview and raster printing.
    pub svg: Option<Arc<resvg::usvg::Tree>>,
    pub has_alpha: bool,
    pub qty: u32,
    pub zoom: f32,
    pub offset_mm: (f32, f32),
    /// Clockwise degrees in -180..=180, about the badge centre (applied after the offset).
    pub rotation: f32,
}

impl Loaded {
    pub fn width(&self) -> u32 {
        self.full.width()
    }
    pub fn height(&self) -> u32 {
        self.full.height()
    }
    pub fn reset_placement(&mut self) {
        self.zoom = 1.0;
        self.offset_mm = (0.0, 0.0);
        self.rotation = 0.0;
    }
}

/// Normalise an angle in degrees to -180..=180.
pub fn wrap_degrees(deg: f32) -> f32 {
    let d = deg.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (dyn_img, svg) = decode(path).map_err(|e| format!("Couldn't open {file}: {e}"))?;
    let has_alpha = dyn_img.color().has_alpha();
    let mut full = dyn_img.into_rgba8();
    if full.width().max(full.height()) > MAX_FULL {
        full = shrink(&full, MAX_FULL);
    }
    let has_alpha = has_alpha && full.pixels().any(|p| p[3] < 255);
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    let full = Arc::new(full);
    // Small images share one buffer rather than keeping an identical preview copy.
    let preview = if full.width().max(full.height()) <= MAX_PREVIEW { Arc::clone(&full) } else { Arc::new(shrink(&full, MAX_PREVIEW)) };
    Ok(Loaded {
        path: path.to_path_buf(),
        name,
        preview,
        full,
        svg,
        has_alpha,
        qty: 0,
        zoom: 1.0,
        offset_mm: (0.0, 0.0),
        rotation: 0.0,
    })
}

fn decode(path: &Path) -> Result<(DynamicImage, Option<Arc<resvg::usvg::Tree>>), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "svg" || ext == "svgz" {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let tree = parse_svg(&bytes)?;
        return Ok((DynamicImage::ImageRgba8(rasterise(&tree)?), Some(Arc::new(tree))));
    }
    let reader = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(friendly)?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(friendly)?;
    img.apply_orientation(orientation);
    Ok((img, None))
}

fn friendly(e: image::ImageError) -> String {
    match e {
        image::ImageError::Unsupported(_) => {
            "Unsupported image format. Try PNG, JPEG, WebP, GIF, TIFF, BMP or SVG (HEIC/AVIF aren't supported).".into()
        }
        other => other.to_string(),
    }
}

/// System fonts for SVG text, scanned once on the first SVG load.
static FONTS: LazyLock<Arc<Database>> = LazyLock::new(|| {
    let mut db = Database::new();
    db.load_system_fonts();
    fill_generic_families(&mut db);
    Arc::new(db)
});

/// Point each generic family (`serif`, `sans-serif`, …) that doesn't resolve at an installed font.
/// fontdb names them after Mac and Windows fonts such as Times New Roman, and usvg falls back to `serif`
/// for any font it can't find, so on a Linux machine without those fonts all SVG text silently vanished.
fn fill_generic_families(db: &mut Database) {
    use resvg::usvg::fontdb::{Family, Query};
    const SERIF: &[&str] = &["DejaVu Serif", "Liberation Serif", "Noto Serif", "FreeSerif"];
    const SANS: &[&str] = &["DejaVu Sans", "Liberation Sans", "Noto Sans", "FreeSans", "Ubuntu", "Cantarell"];
    const MONO: &[&str] = &["DejaVu Sans Mono", "Liberation Mono", "Noto Sans Mono", "FreeMono"];
    type Generic = (Family<'static>, &'static [&'static [&'static str]], fn(&mut Database, String));
    let generics: [Generic; 5] = [
        (Family::Serif, &[SERIF, SANS], |db, f| db.set_serif_family(f)),
        (Family::SansSerif, &[SANS, SERIF], |db, f| db.set_sans_serif_family(f)),
        (Family::Monospace, &[MONO, SANS, SERIF], |db, f| db.set_monospace_family(f)),
        (Family::Cursive, &[SANS, SERIF], |db, f| db.set_cursive_family(f)),
        (Family::Fantasy, &[SANS, SERIF], |db, f| db.set_fantasy_family(f)),
    ];
    let resolves = |db: &Database, family| db.query(&Query { families: &[family], ..Default::default() }).is_some();
    // Last resort: whatever font is installed, so text still shows, if not in the intended style.
    let Some(any) = db.faces().find_map(|f| f.families.first()).map(|(name, _)| name.clone()) else {
        return;
    };
    for (generic, candidates, set) in generics {
        if !resolves(db, generic) {
            let found = candidates.iter().flat_map(|c| c.iter()).find(|name| resolves(db, Family::Name(name)));
            set(db, found.map_or_else(|| any.clone(), |name| name.to_string()));
        }
    }
}

fn parse_svg(bytes: &[u8]) -> Result<resvg::usvg::Tree, String> {
    parse_svg_with(bytes, Arc::clone(&FONTS))
}

fn parse_svg_with(bytes: &[u8], fontdb: Arc<Database>) -> Result<resvg::usvg::Tree, String> {
    let options = resvg::usvg::Options { fontdb, ..Default::default() };
    resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| e.to_string())
}

/// Bitmap of an SVG for the preview and raster printing (the PDF embeds the tree itself).
fn rasterise(tree: &resvg::usvg::Tree) -> Result<RgbaImage, String> {
    use resvg::tiny_skia::{Pixmap, Transform};
    let size = tree.size();
    let scale = 2048.0 / size.width().max(size.height()).max(1.0);
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = Pixmap::new(w.max(1), h.max(1)).ok_or("SVG too large")?;
    resvg::render(tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let mut out = RgbaImage::new(pixmap.width(), pixmap.height());
    for (dst, src) in out.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        *dst = Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(out)
}

/// Downscale so the long edge is at most `max` px (returns a copy either way).
pub fn shrink(img: &RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max {
        return img.clone();
    }
    let s = max as f32 / long as f32;
    imageops::resize(img, ((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1), FilterType::Lanczos3)
}

/// Composite one badge as a square bitmap `px` pixels across covering the cut circle:
/// background disc, the image placed per the sheet's fit/zoom/offset/rotation, clipped to the cut circle.
pub fn compose(sheet: &Sheet, img: &Loaded, src: &RgbaImage, px: u32) -> RgbaImage {
    let px = px.max(2);
    let ppm = px as f32 / sheet.cut_mm().max(0.1);
    let c = px as f32 / 2.0;
    // Anti-aliased coverage of the cut circle at a pixel.
    let coverage = |x: u32, y: u32| (c + 0.5 - (x as f32 + 0.5 - c).hypot(y as f32 + 0.5 - c)).clamp(0.0, 1.0);
    let mut canvas = RgbaImage::new(px, px);

    if let Some(bg) = sheet.background {
        let [r, g, b] = bg.map(|v| (v * 255.0) as u8);
        for (x, y, p) in canvas.enumerate_pixels_mut() {
            *p = Rgba([r, g, b, (coverage(x, y) * 255.0) as u8]);
        }
    }

    // Size comes from the full image so preview (small `src`) and print agree.
    let rect = sheet.image_rect(img.width(), img.height(), img.zoom, img.offset_mm);
    let dw = ((rect.w * ppm).round() as u32).clamp(1, 8192);
    let dh = ((rect.h * ppm).round() as u32).clamp(1, 8192);
    let scaled = if (dw, dh) == src.dimensions() {
        Cow::Borrowed(src)
    } else {
        let filter = if dw < src.width() { FilterType::Triangle } else { FilterType::CatmullRom };
        Cow::Owned(imageops::resize(src, dw, dh, filter))
    };
    let (x, y) = (c + rect.x * ppm, c + rect.y * ppm);
    if img.rotation == 0.0 {
        imageops::overlay(&mut canvas, &*scaled, x.round() as i64, y.round() as i64);
    } else {
        draw_rotated(&mut canvas, &scaled, (x, y), c, img.rotation);
    }

    if sheet.guides.clip {
        for (x, y, p) in canvas.enumerate_pixels_mut() {
            p[3] = (p[3] as f32 * coverage(x, y)) as u8;
        }
    }
    canvas
}

/// Blend `src` (top-left at `pos`) onto `canvas`, rotated `deg` clockwise about (`pivot`, `pivot`). Bilinear.
fn draw_rotated(canvas: &mut RgbaImage, src: &RgbaImage, pos: (f32, f32), pivot: f32, deg: f32) {
    let (sin, cos) = deg.to_radians().sin_cos();
    let (w, h) = (src.width() as i64, src.height() as i64);
    // Premultiplied texel, transparent outside the image (which anti-aliases its edges).
    let texel = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= w || y >= h {
            return [0.0; 4];
        }
        let p = src.get_pixel(x as u32, y as u32);
        let a = p[3] as f32 / 255.0;
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
    };
    for (x, y, out) in canvas.enumerate_pixels_mut() {
        // Inverse-rotate the pixel centre into the unrotated image's pixel space.
        let (dx, dy) = (x as f32 + 0.5 - pivot, y as f32 + 0.5 - pivot);
        let u = dx * cos + dy * sin + pivot - pos.0 - 0.5;
        let v = -dx * sin + dy * cos + pivot - pos.1 - 0.5;
        if u <= -1.0 || v <= -1.0 || u >= w as f32 || v >= h as f32 {
            continue;
        }
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut s = [0.0f32; 4];
        for (tx, ty, wt) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x0 + 1, y0, fx * (1.0 - fy)), (x0, y0 + 1, (1.0 - fx) * fy), (x0 + 1, y0 + 1, fx * fy)] {
            let t = texel(tx, ty);
            for k in 0..4 {
                s[k] += t[k] * wt;
            }
        }
        if s[3] <= 0.0 {
            continue;
        }
        // Source-over onto the straight-alpha canvas.
        let da = out[3] as f32 / 255.0 * (1.0 - s[3]);
        let oa = s[3] + da;
        for k in 0..3 {
            out[k] = ((s[k] + out[k] as f32 * da) / oa).round() as u8;
        }
        out[3] = (oa * 255.0).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_text_is_drawn() {
        if FONTS.is_empty() {
            return; // a machine with no fonts installed can't draw text
        }
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60"><text x="10" y="40" font-size="32">Badger</text></svg>"#;
        let tree = parse_svg(svg).unwrap();
        assert!(rasterise(&tree).unwrap().pixels().any(|p| p[3] > 0), "text left no pixels");
    }

    #[test]
    fn svg_text_is_drawn_when_no_generic_family_is_installed() {
        // Like a Linux machine without Times New Roman and co., which fontdb's generic families default to.
        let mut db = (**FONTS).clone();
        if db.is_empty() {
            return;
        }
        db.set_serif_family("No Such Font");
        db.set_sans_serif_family("No Such Font");
        db.set_monospace_family("No Such Font");
        db.set_cursive_family("No Such Font");
        db.set_fantasy_family("No Such Font");
        fill_generic_families(&mut db);
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60"><text x="10" y="40" font-size="32" font-family="No Such Font">Badger</text></svg>"#;
        let tree = parse_svg_with(svg, Arc::new(db)).unwrap();
        assert!(rasterise(&tree).unwrap().pixels().any(|p| p[3] > 0), "text left no pixels");
    }

    #[test]
    fn rotating_90_matches_rotated_pixels() {
        // Rotating the placement must look like rotating the pixels. The image is wide and lopsided both ways,
        // so a turn the wrong way, a flip or a shift of one pixel would each show.
        let src = RgbaImage::from_fn(60, 30, |x, y| Rgba(if x < 20 { [255, 0, 0, 255] } else if y < 10 { [0, 255, 0, 255] } else { [0, 0, 255, 255] }));
        let img = |pixels: RgbaImage, rotation| Loaded {
            path: PathBuf::new(),
            name: String::new(),
            full: Arc::new(pixels.clone()),
            preview: Arc::new(pixels),
            svg: None,
            has_alpha: false,
            qty: 0,
            zoom: 1.0,
            offset_mm: (0.0, 0.0),
            rotation,
        };
        let sheet = Sheet { guides: crate::layout::Guides { clip: false, ..Default::default() }, ..Sheet::default() };
        let a = img(src.clone(), 90.0);
        let b = img(imageops::rotate90(&src), 0.0);
        let (ra, rb) = (compose(&sheet, &a, &a.full, 100), compose(&sheet, &b, &b.full, 100));
        let worst = ra.pixels().zip(rb.pixels()).flat_map(|(p, q)| (0..4).map(move |k| p[k].abs_diff(q[k]))).max().unwrap();
        assert!(worst <= 1, "a pixel differs by {worst}/255"); // only rounding may disagree
    }
}
