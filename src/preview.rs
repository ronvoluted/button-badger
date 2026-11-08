//! The live page preview: a canvas for paper + images with pan/zoom and image dragging,
//! plus a second canvas (stacked above, in its own render layer) for guides and hover rings.
//! Two canvases are needed because iced draws all vector geometry of a layer beneath its images.

use crate::images::{Loaded, wrap_degrees};
use crate::layout::{CROP_TICK_END, CROP_TICK_START, LineStyle, PT_PER_MM, Sheet};
use crate::theme;
use iced::mouse;
use iced::widget::canvas::{Action, Cache, Frame, Geometry, Image, LineCap, LineDash, Path, Program, Stroke, Text, path};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Event, Point, Radians, Rectangle, Renderer, Size, Theme, Vector, keyboard};

const PAD: f32 = 36.0;

/// Where the page sits in the canvas; shared by both canvases.
#[derive(Clone, Copy)]
pub struct Viewport<'a> {
    pub sheet: &'a Sheet,
    /// Badge centres on a page (mm), i.e. `sheet.cells()`, computed once per layout change.
    pub cells: &'a [(f32, f32)],
    /// Pixels per mm; `None` = fit to the canvas.
    pub zoom: Option<f32>,
    pub pan: Vector,
}

/// mm → canvas pixels for one frame.
#[derive(Clone, Copy)]
struct Screen {
    origin: Point,
    scale: f32,
}

impl Screen {
    fn point(self, (x, y): (f32, f32)) -> Point {
        Point::new(self.origin.x + x * self.scale, self.origin.y + y * self.scale)
    }
}

impl Viewport<'_> {
    fn scale(&self, size: Size) -> f32 {
        self.zoom.unwrap_or_else(|| {
            let (w, h) = self.sheet.page_mm();
            ((size.width - 2.0 * PAD) / w).min((size.height - 2.0 * PAD) / h).max(0.05)
        })
    }

    fn origin(&self, size: Size, s: f32) -> Point {
        let (w, h) = self.sheet.page_mm();
        Point::new(size.width / 2.0 - w * s / 2.0 + self.pan.x, size.height / 2.0 - h * s / 2.0 + self.pan.y)
    }

    fn screen(&self, size: Size) -> Screen {
        let scale = self.scale(size);
        Screen { origin: self.origin(size, scale), scale }
    }

    fn cut_radius_px(&self, size: Size) -> f32 {
        self.sheet.cut_mm() / 2.0 * self.scale(size)
    }

    fn cell_at(&self, size: Size, pos: Point) -> Option<usize> {
        let screen = self.screen(size);
        let r = self.cut_radius_px(size);
        self.cells.iter().position(|&c| pos.distance(screen.point(c)) <= r)
    }
}

pub struct Preview<'a> {
    pub view: Viewport<'a>,
    pub images: &'a [Loaded],
    /// Pre-composited badge bitmap per image index (square, covers the cut circle).
    pub badges: &'a [Handle],
    /// Image index per cell on the page being shown.
    pub items: &'a [Option<usize>],
    pub cache: &'a Cache,
}

#[derive(Debug, Clone)]
pub enum Interaction {
    /// New zoom (px per mm) and pan that keep the point under the cursor fixed.
    Zoom(f32, Vector),
    Pan(Vector),
    ImageOffset(usize, (f32, f32)),
    ImageZoom(usize, f32),
    Hover(Option<usize>),
    /// Scale handle being dragged: new zoom for the image.
    ScaleDrag(usize, f32),
    /// Rotate handle being dragged: new rotation (degrees) for the image.
    RotateDrag(usize, f32),
    DragEnd,
}

#[derive(Default)]
pub struct State {
    drag: Option<Drag>,
    hover: Option<usize>,
    alt: bool,
    shift: bool,
}

enum Drag {
    Pan { last: Point },
    Image { image: usize, start: (f32, f32), from: Point },
    Scale { image: usize, start_zoom: f32, centre: Point, start_dist: f32 },
    Rotate { image: usize, start_rotation: f32, centre: Point, start_angle: f32 },
}

/// The two handles shown on the hovered badge's ring.
#[derive(Clone, Copy, PartialEq)]
enum Grip {
    Scale,
    Rotate,
}

const HANDLE_RADIUS: f32 = 7.0;

/// Where a handle sits for a cell: on the ring, bottom-right (scale) or top-right (rotate).
fn handle_point(centre: Point, cut_r: f32, grip: Grip) -> Point {
    let d = (cut_r + 3.0) * std::f32::consts::FRAC_1_SQRT_2;
    Point::new(centre.x + d, if grip == Grip::Scale { centre.y + d } else { centre.y - d })
}

fn angle(centre: Point, p: Point) -> f32 {
    (p.y - centre.y).atan2(p.x - centre.x)
}

impl Preview<'_> {
    fn image_at(&self, cell: usize) -> Option<usize> {
        self.items.get(cell).copied().flatten().filter(|&i| i < self.images.len())
    }

    /// If `pos` is on one of the hovered cell's handles: (handle, image, cell centre).
    fn handle_hit(&self, state: &State, size: Size, pos: Point) -> Option<(Grip, usize, Point)> {
        let cell = state.hover?;
        let image = self.image_at(cell)?;
        let centre = self.view.screen(size).point(*self.view.cells.get(cell)?);
        let r = self.view.cut_radius_px(size);
        [Grip::Scale, Grip::Rotate].into_iter().find(|&g| pos.distance(handle_point(centre, r, g)) <= HANDLE_RADIUS + 3.0).map(|g| (g, image, centre))
    }
}

impl Program<Interaction> for Preview<'_> {
    type State = State;

    fn update(&self, state: &mut State, event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<Interaction>> {
        let size = bounds.size();
        let v = self.view;
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.alt = m.alt();
                state.shift = m.shift();
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let pos = cursor.position_in(bounds);
                match (&state.drag, pos) {
                    (Some(Drag::Pan { last }), Some(pos)) => {
                        let delta = pos - *last;
                        state.drag = Some(Drag::Pan { last: pos });
                        Some(Action::publish(Interaction::Pan(v.pan + delta)).and_capture())
                    }
                    (Some(Drag::Image { image, start, from }), Some(pos)) => {
                        // The offset lives in the image's unrotated frame, so un-rotate the screen delta.
                        let s = v.scale(size);
                        let d = pos - *from;
                        let (sin, cos) = self.images[*image].rotation.to_radians().sin_cos();
                        let offset = (start.0 + (d.x * cos + d.y * sin) / s, start.1 + (d.y * cos - d.x * sin) / s);
                        Some(Action::publish(Interaction::ImageOffset(*image, offset)).and_capture())
                    }
                    (Some(Drag::Scale { image, start_zoom, centre, start_dist }), Some(pos)) => {
                        let dist = pos.distance(*centre).max(1.0);
                        let zoom = (start_zoom * dist / start_dist).clamp(0.05, 20.0);
                        Some(Action::publish(Interaction::ScaleDrag(*image, zoom)).and_capture())
                    }
                    (Some(Drag::Rotate { image, start_rotation, centre, start_angle }), Some(pos)) => {
                        let deg = start_rotation + (angle(*centre, pos) - start_angle).to_degrees();
                        // Shift snaps to 15°; otherwise right angles are sticky.
                        let right = (deg / 90.0).round() * 90.0;
                        let deg = if state.shift {
                            (deg / 15.0).round() * 15.0
                        } else if (deg - right).abs() < 3.0 {
                            right
                        } else {
                            deg
                        };
                        Some(Action::publish(Interaction::RotateDrag(*image, wrap_degrees(deg))).and_capture())
                    }
                    (None, pos) => {
                        if let Some(p) = pos
                            && self.handle_hit(state, size, p).is_some()
                        {
                            return None; // stay on the current cell while over its handle
                        }
                        let hover = pos.and_then(|p| v.cell_at(size, p)).filter(|&c| self.image_at(c).is_some());
                        if hover != state.hover {
                            state.hover = hover;
                            Some(Action::publish(Interaction::Hover(hover)))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                if let Some((grip, image, centre)) = self.handle_hit(state, size, pos) {
                    let img = &self.images[image];
                    state.drag = Some(match grip {
                        Grip::Scale => Drag::Scale { image, start_zoom: img.zoom, centre, start_dist: pos.distance(centre).max(1.0) },
                        Grip::Rotate => Drag::Rotate { image, start_rotation: img.rotation, centre, start_angle: angle(centre, pos) },
                    });
                    return Some(Action::capture());
                }
                let image = v.cell_at(size, pos).and_then(|c| self.image_at(c));
                state.drag = Some(match image {
                    Some(i) => Drag::Image { image: i, start: self.images[i].offset_mm, from: pos },
                    None => Drag::Pan { last: pos },
                });
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right | mouse::Button::Middle)) => {
                let pos = cursor.position_in(bounds)?;
                state.drag = Some(Drag::Pan { last: pos });
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left | mouse::Button::Right | mouse::Button::Middle)) => {
                state.drag.take().map(|_| Action::publish(Interaction::DragEnd))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let pos = cursor.position_in(bounds)?;
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 0.12,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 160.0,
                };
                let factor = (1.0 + dy).clamp(0.5, 2.0);
                if state.alt && let Some(i) = v.cell_at(size, pos).and_then(|c| self.image_at(c)) {
                    let zoom = (self.images[i].zoom * factor).clamp(0.1, 20.0);
                    return Some(Action::publish(Interaction::ImageZoom(i, zoom)).and_capture());
                }
                let s = v.scale(size);
                let s2 = (s * factor).clamp(0.2, 60.0);
                let o = v.origin(size, s);
                let mm = Point::new((pos.x - o.x) / s, (pos.y - o.y) / s);
                let o2 = Point::new(pos.x - mm.x * s2, pos.y - mm.y * s2);
                let (w, h) = v.sheet.page_mm();
                let pan = Vector::new(o2.x - size.width / 2.0 + w * s2 / 2.0, o2.y - size.height / 2.0 + h * s2 / 2.0);
                Some(Action::publish(Interaction::Zoom(s2, pan)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &State, renderer: &Renderer, theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let c = theme::colors(theme);
        let size = bounds.size();
        let v = self.view;
        let screen = v.screen(size);
        let (w_mm, h_mm) = v.sheet.page_mm();
        let page = Rectangle::new(screen.origin, Size::new(w_mm * screen.scale, h_mm * screen.scale));
        let cut_r = v.cut_radius_px(size);

        let geometry = self.cache.draw(renderer, size, |frame| {
            // Soft shadow + paper.
            for (i, a) in [(14.0, 0.05), (8.0, 0.07), (3.0, 0.10)] {
                let r = Rectangle::new(Point::new(page.x - i, page.y - i + 4.0), Size::new(page.width + 2.0 * i, page.height + 2.0 * i));
                frame.fill(&Path::rounded_rectangle(r.position(), r.size(), (i + 2.0).into()), Color { a, ..c.shadow });
            }
            frame.fill_rectangle(page.position(), page.size(), Color::WHITE);

            for (n, &cell) in v.cells.iter().enumerate() {
                let centre = screen.point(cell);
                match self.image_at(n).and_then(|i| self.badges.get(i)) {
                    Some(handle) => {
                        let r = Rectangle::new(Point::new(centre.x - cut_r, centre.y - cut_r), Size::new(cut_r * 2.0, cut_r * 2.0));
                        frame.draw_image(r, Image::new(handle.clone()).filter_method(FilterMethod::Linear));
                    }
                    None => {
                        if let Some(bg) = v.sheet.background {
                            frame.fill(&Path::circle(centre, cut_r), Color::from_rgb(bg[0], bg[1], bg[2]));
                        }
                    }
                }
            }

            let caption = |content: &str, color, size: f32| Text {
                content: content.into(),
                position: page.center(),
                color,
                size: size.into(),
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            };
            if v.cells.is_empty() {
                frame.fill_text(caption("Nothing fits — reduce the gap, margin or badge size", c.danger, 15.0));
            } else if self.images.is_empty() {
                frame.fill_text(caption("Drop an image here\nor press ⌘O to open one", c.muted, 16.0 * (screen.scale / 3.0).clamp(0.8, 1.6)));
            }
        });
        vec![geometry]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match &state.drag {
            Some(Drag::Scale { .. }) => mouse::Interaction::ResizingDiagonallyDown,
            Some(_) => mouse::Interaction::Grabbing,
            None => {
                match cursor.position_in(bounds).and_then(|p| self.handle_hit(state, bounds.size(), p)) {
                    Some((Grip::Scale, ..)) => return mouse::Interaction::ResizingDiagonallyDown,
                    Some((Grip::Rotate, ..)) => return mouse::Interaction::Pointer,
                    None => {}
                }
                let over_image = cursor
                    .position_in(bounds)
                    .and_then(|p| self.view.cell_at(bounds.size(), p))
                    .and_then(|c| self.image_at(c))
                    .is_some();
                if over_image { mouse::Interaction::Grab } else { mouse::Interaction::default() }
            }
        }
    }
}

/// Guides and highlights, drawn above the images.
pub struct Guides<'a> {
    pub view: Viewport<'a>,
    pub items: &'a [Option<usize>],
    pub cache: &'a Cache,
    /// Hovered cell (ring) and image being dragged (rings on every copy).
    pub hover: Option<usize>,
    pub dragging: Option<usize>,
}

impl Program<Interaction> for Guides<'_> {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let c = theme::colors(theme);
        let size = bounds.size();
        let v = self.view;
        let screen = v.screen(size);
        let s = screen.scale;
        let cut_r = v.cut_radius_px(size);

        let guides = self.cache.draw(renderer, size, |frame| {
            // Printable area hint.
            let pr = v.sheet.printable();
            frame.stroke(
                &Path::rectangle(screen.point((pr.x, pr.y)), Size::new(pr.w * s, pr.h * s)),
                Stroke { style: Color { a: 0.5, ..c.faint }.into(), width: 1.0, line_dash: LineDash { segments: &[4.0, 4.0], offset: 0 }, ..Default::default() },
            );

            let g = &v.sheet.guides;
            let lw = (g.weight_pt / PT_PER_MM * s).max(0.75);
            let gray = Color::from_rgb(g.gray, g.gray, g.gray);
            for &cell in v.cells {
                let centre = screen.point(cell);
                let circle = Path::circle(centre, cut_r);
                match g.cut {
                    LineStyle::None => {}
                    LineStyle::Dotted => frame.stroke(
                        &circle,
                        Stroke { style: gray.into(), width: lw, line_cap: LineCap::Round, line_dash: LineDash { segments: &[lw * 0.25, lw * 2.75], offset: 0 }, ..Default::default() },
                    ),
                    LineStyle::Dashed => frame.stroke(
                        &circle,
                        Stroke { style: gray.into(), width: lw, line_dash: LineDash { segments: &[lw * 6.0, lw * 4.0], offset: 0 }, ..Default::default() },
                    ),
                    LineStyle::Solid => frame.stroke(&circle, Stroke { style: gray.into(), width: lw, ..Default::default() }),
                }
                if g.face {
                    // Framing aid: preview only, never printed.
                    frame.stroke(
                        &Path::circle(centre, v.sheet.face_mm / 2.0 * s),
                        Stroke { style: Color { a: 0.85, ..c.accent }.into(), width: 1.0, line_dash: LineDash { segments: &[3.0, 3.0], offset: 0 }, ..Default::default() },
                    );
                }
                if g.crop_marks {
                    let (a, b) = (cut_r + CROP_TICK_START * s, cut_r + CROP_TICK_END * s);
                    let st = Stroke { style: gray.into(), width: lw, ..Default::default() };
                    for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                        frame.stroke(&Path::line(Point::new(centre.x + dx * a, centre.y + dy * a), Point::new(centre.x + dx * b, centre.y + dy * b)), st);
                    }
                }
            }
        });

        // Hover/drag highlights change often, so they skip the cache.
        let mut overlay = Frame::new(renderer, size);
        let ring = |frame: &mut Frame, cell: (f32, f32), color: Color| {
            frame.stroke(&Path::circle(screen.point(cell), cut_r + 3.0), Stroke { style: color.into(), width: 2.0, ..Default::default() });
        };
        let hovered = self.hover.and_then(|h| v.cells.get(h)).copied();
        if let Some(img) = self.dragging {
            for (n, &cell) in v.cells.iter().enumerate() {
                if self.items.get(n).copied().flatten() == Some(img) {
                    ring(&mut overlay, cell, Color { a: 0.35, ..c.accent });
                }
            }
        } else if let Some(cell) = hovered {
            ring(&mut overlay, cell, Color { a: 0.7, ..c.accent });
        }
        if let Some(cell) = hovered {
            let centre = screen.point(cell);
            let h = handle_point(centre, cut_r, Grip::Scale);
            overlay.fill(&Path::circle(h, HANDLE_RADIUS), Color::WHITE);
            overlay.fill(&Path::circle(h, HANDLE_RADIUS - 2.0), c.accent);
            // Rotate: a white disc with an open accent arc, so it reads differently from the solid scale dot.
            let h = handle_point(centre, cut_r, Grip::Rotate);
            overlay.fill(&Path::circle(h, HANDLE_RADIUS), Color::WHITE);
            let arc = Path::new(|p| p.arc(path::Arc { center: h, radius: HANDLE_RADIUS - 3.0, start_angle: Radians(-1.2), end_angle: Radians(4.0) }));
            overlay.stroke(&arc, Stroke { style: c.accent.into(), width: 1.75, line_cap: LineCap::Round, ..Default::default() });
        }
        vec![guides, overlay.into_geometry()]
    }
}
