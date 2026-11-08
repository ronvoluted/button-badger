//! Renders packaging/icon.svg into a macOS .iconset folder, plus packaging/icon-256.png (window icon)
//! and packaging/icon.ico (Windows .exe icon, embedded by build.rs).
//! Usage: cargo run --release --example render_icon -- packaging/icon.svg target/AppIcon.iconset

use image::ExtendedColorType;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

fn main() {
    let mut args = std::env::args().skip(1);
    let svg = args.next().expect("svg path");
    let out = std::path::PathBuf::from(args.next().expect("output .iconset dir"));
    std::fs::create_dir_all(&out).unwrap();
    let tree = Tree::from_data(&std::fs::read(&svg).unwrap(), &Options::default()).unwrap();
    let base = tree.size().width();
    let render = |px: u32| {
        let mut pixmap = Pixmap::new(px, px).unwrap();
        let s = px as f32 / base;
        resvg::render(&tree, Transform::from_scale(s, s), &mut pixmap.as_mut());
        pixmap.encode_png().unwrap()
    };
    for (size, scale) in [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)] {
        let name = if scale == 1 { format!("icon_{size}x{size}.png") } else { format!("icon_{size}x{size}@2x.png") };
        std::fs::write(out.join(name), render(size * scale)).unwrap();
    }
    std::fs::write("packaging/icon-256.png", render(256)).unwrap();
    let frames: Vec<IcoFrame> = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|px| IcoFrame::with_encoded(render(px), px, px, ExtendedColorType::Rgba8).unwrap())
        .collect();
    IcoEncoder::new(std::fs::File::create("packaging/icon.ico").unwrap()).encode_images(&frames).unwrap();
    println!("wrote {}", out.display());
}
