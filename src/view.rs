//! The window's widgets: sidebar cards, the preview workspace and small styled helpers.

use crate::layout::{Fit, LineStyle, Orientation, PRESETS, Packing, Paper, TestPage, Units, trim_float};
use crate::preview::{Guides, Preview, Viewport};
use crate::theme::{self, Colors, SIDEBAR_WIDTH, SPACE_M, SPACE_S, SPACE_XS, icon};
use crate::{App, Bg, CUSTOM_BADGE, Field, Message, Toast, Toggle};

use iced::widget::{Button, Column, Row, button, canvas, column, container, pick_list, row, rule, scrollable, slider, space, stack, text, text_input, toggler, tooltip};
use iced::{Alignment, Color, Element, Font, Length::Fill, Padding, Theme, font};

const SEMIBOLD: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };

impl App {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        row![self.sidebar(), self.workspace()].into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let header = row![
            text("Button Badger").size(19).font(SEMIBOLD),
            space().width(Fill),
            tip(
                icon_button(if self.dark { theme::SUN } else { theme::MOON }, 16.0, 6, Some(Message::ToggleDark)),
                if self.dark { "Switch to Light mode" } else { "Switch to Dark mode" }
            ),
        ]
        .align_y(Alignment::Center);

        let content = column![
            header,
            self.images_card(),
            self.badge_card(),
            self.paper_card(),
            self.guides_card(),
            self.presets_card(),
            self.calibration_card(),
            tip(
                label_button(theme::UNDO, |c| c.muted, "Reset to defaults").style(theme::ghost).padding([6, 10]).on_press(Message::ResetDefaults),
                "Badge, paper and guide settings. Images, saved setups and calibration are kept.",
            ),
            text("Scroll to zoom · drag image to reposition · drag the ring handles to scale or rotate (⇧ snaps)").size(11).style(faint),
        ]
        .spacing(SPACE_M)
        .padding(SPACE_M);

        container(scrollable(content).width(SIDEBAR_WIDTH).height(Fill).spacing(2)).style(theme::sidebar).height(Fill).into()
    }

    fn images_card(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(SPACE_S).push(section("Images"));
        for (i, img) in self.images.iter().enumerate() {
            let thumb = tip(
                button(iced::widget::image(self.badges[i].clone()).width(36).height(36)).style(theme::ghost).padding(2).on_press(Message::Replace(i)),
                "Replace image",
            );
            let dpi = self.sheet().effective_dpi(img.width(), img.height(), img.zoom);
            let low = dpi < 150.0;
            let meta = text(format!("{}×{} · {} dpi", img.width(), img.height(), dpi.round()))
                .size(11)
                .wrapping(text::Wrapping::None)
                .style(move |t: &Theme| tone(t, if low { |c| c.warn } else { |c| c.muted }));
            let meta: Element<'_, Message> = if low { tip(meta, "Low resolution for this badge size") } else { meta.into() };
            let name = container(column![text(truncate_middle(&img.name, 30)).size(13).wrapping(text::Wrapping::None), meta].spacing(2)).clip(true).width(Fill);

            let header = row![
                thumb,
                name,
                tip(icon_button(theme::UNDO, 13.0, 4, Some(Message::ResetPlacement(i))), "Reset position, scale & rotation"),
                tip(icon_button(theme::TRASH, 13.0, 4, Some(Message::Remove(i))), "Remove"),
            ]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center);
            let field = |label, f: Field, suffix, hint| {
                tip(column![text(label).size(12).style(muted), stepper(self.text(f), f, 58.0, suffix)].spacing(4).width(Fill), hint)
            };
            let controls = row![
                field("Copies", Field::Copies(i), None, "Copies (0 = fill the rest)"),
                field("Scale", Field::Scale(i), Some("%"), "Image scale"),
                field("Rotation", Field::Rotation(i), Some("°"), "Rotate: type an angle, or use the arrows for 90° turns"),
            ]
            .spacing(SPACE_S);

            col = col.push(column![header, controls].spacing(SPACE_XS));
            if i + 1 < self.images.len() {
                col = col.push(rule::horizontal(1).style(|t: &Theme| rule::Style {
                    color: theme::colors(t).border,
                    radius: 0.0.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: true,
                }));
            }
        }
        let add_label = if self.loading > 0 { "Loading…" } else { "Add image" };
        col = col.push(label_button(theme::PLUS, |c| c.text, add_label).style(theme::secondary).padding([7, 12]).width(Fill).on_press(Message::Open));

        if self.images.is_empty() && !self.settings.recent.is_empty() {
            let mut recents = Column::new().spacing(2).push(text("Recent").size(11).style(muted));
            for p in self.settings.recent.iter().take(4) {
                let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let link = button(text(truncate_middle(&label, 38)).size(12).wrapping(text::Wrapping::None)).style(theme::link).padding([2, 0]).on_press(Message::OpenRecent(p.clone()));
                recents = recents.push(tip(link, label));
            }
            col = col.push(recents);
        }
        card(col)
    }

    fn badge_card(&self) -> Element<'_, Message> {
        let s = self.sheet();
        let u = self.units();
        let preset_names: Vec<String> = PRESETS.iter().map(|p| p.name.to_string()).chain([CUSTOM_BADGE.to_string()]).collect();
        let selected = PRESETS
            .iter()
            .find(|p| (p.face_mm - s.face_mm).abs() < 0.01 && (p.bleed_mm - s.bleed_mm).abs() < 0.01)
            .map_or(CUSTOM_BADGE, |p| p.name)
            .to_string();

        let mut background = Column::new().spacing(SPACE_XS).push(segmented(&[(Bg::None, "Paper"), (Bg::White, "White"), (Bg::Custom, "Colour")], self.bg_mode, Message::Background));
        if self.bg_mode == Bg::Custom {
            let current = s.background.unwrap_or([1.0; 3]);
            let swatches = SWATCHES.iter().fold(Row::new().spacing(SPACE_XS).align_y(Alignment::Center), |r, &rgb| {
                let selected = rgb.iter().zip(current).all(|(a, b)| (a - b).abs() < 0.002);
                r.push(button(space().width(16).height(16)).padding(2).style(theme::swatch(rgb, selected)).on_press(Message::BgSwatch(rgb)))
            });
            let hex = row![
                text("Hex").size(12).style(muted),
                space().width(Fill),
                text_input("#RRGGBB", &self.bg_hex).on_input(Message::BgHex).width(90).padding([4, 6]).size(12).style(theme::field),
            ]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center);
            background = background.push(swatches).push(hex);
        }

        card(
            column![
                section("Badge"),
                pick_list(preset_names, Some(selected), Message::BadgePreset).width(Fill).padding([6, 10]).text_size(13).style(theme::picker),
                row![
                    num_field("Face", self.text(Field::Face), u.suffix(), Field::Face),
                    num_field("Bleed", self.text(Field::Bleed), u.suffix(), Field::Bleed),
                ]
                .spacing(SPACE_S),
                row![
                    text("Cut circle").size(12).style(muted),
                    space().width(Fill),
                    container(text(format!("Ø {} {}", u.format(s.cut_mm()), u.suffix())).size(12)).style(theme::chip).padding([2, 8]),
                ]
                .align_y(Alignment::Center),
                labelled("Image fit", segmented(&[(Fit::Fill, "Cover"), (Fit::Fit, "Contain")], s.fit, Message::Fit)),
                labelled("Background", background),
                switch("Clip image", s.guides.clip, Toggle::Clip),
            ]
            .spacing(SPACE_S),
        )
    }

    fn paper_card(&self) -> Element<'_, Message> {
        let s = self.sheet();
        let u = self.units();
        let mut col = column![
            section("Paper"),
            row![
                pick_list(Paper::ALL.as_slice(), Some(s.paper), Message::Paper).width(Fill).padding([6, 10]).text_size(13).style(theme::picker),
                segmented(&[(Units::Mm, "mm"), (Units::Inch, "in")], u, Message::Units),
            ]
            .spacing(SPACE_S)
            .align_y(Alignment::Center),
        ]
        .spacing(SPACE_S);
        if s.paper == Paper::Custom {
            col = col.push(
                row![
                    num_field("Width", self.text(Field::PaperW), u.suffix(), Field::PaperW),
                    num_field("Height", self.text(Field::PaperH), u.suffix(), Field::PaperH),
                ]
                .spacing(SPACE_S),
            );
        }
        card(
            col.push(segmented(&[(Orientation::Portrait, "Portrait"), (Orientation::Landscape, "Landscape")], s.orientation, Message::Orientation))
                .push(slider_field("Gap between badges", Field::Gap, s.gap_mm, 0.0..=20.0, self.text(Field::Gap), u))
                .push(slider_field("Printer margin", Field::Margin, s.margin_mm, 0.0..=30.0, self.text(Field::Margin), u))
                .push(labelled("Packing", segmented(&[(Packing::Auto, "Max"), (Packing::Grid, "Grid"), (Packing::Hex, "Hex")], s.packing, Message::Packing))),
        )
    }

    fn guides_card(&self) -> Element<'_, Message> {
        let g = &self.sheet().guides;
        let cut_styles = [(LineStyle::Dotted, "Dotted"), (LineStyle::Dashed, "Dashed"), (LineStyle::Solid, "Solid"), (LineStyle::None, "Off")];
        card(
            column![
                section("Guides"),
                labelled("Cut line", segmented(&cut_styles, g.cut, Message::CutStyle)),
                row![
                    column![
                        tip(switch("Framing aid", g.face, Toggle::FaceLine), "Shows the face circle in the preview so you can frame the art. It is never printed."),
                        switch("Crop ticks", g.crop_marks, Toggle::CropMarks),
                    ]
                    .spacing(SPACE_S)
                    .width(Fill),
                    labelled("Line weight", stepper(self.text(Field::Weight), Field::Weight, 60.0, Some("pt"))),
                ]
                .spacing(SPACE_S),
            ]
            .spacing(SPACE_S),
        )
    }

    fn presets_card(&self) -> Element<'_, Message> {
        let mut col = column![section("Saved setups")].spacing(SPACE_S);
        if !self.settings.presets.is_empty() {
            let names: Vec<String> = self.settings.presets.iter().map(|p| p.name.clone()).collect();
            col = col.push(
                row![
                    pick_list(names, self.selected_preset.clone(), Message::LoadPreset).placeholder("Load a setup").width(Fill).padding([6, 10]).text_size(13).style(theme::picker),
                    tip(icon_button(theme::TRASH, 14.0, 6, self.selected_preset.as_ref().map(|_| Message::DeletePreset)), "Delete selected setup"),
                ]
                .spacing(SPACE_XS)
                .align_y(Alignment::Center),
            );
        }
        let can_save = !self.preset_name.trim().is_empty();
        card(
            col.push(
                row![
                    text_input("Enter a name", &self.preset_name).on_input(Message::PresetName).on_submit(Message::SavePreset).padding([6, 10]).size(13).style(theme::field),
                    tip(
                        button(icon(theme::BOOKMARK, 14.0, |c| c.text)).style(theme::secondary).padding(7).on_press_maybe(can_save.then_some(Message::SavePreset)),
                        "Save badge, paper and guide settings"
                    ),
                ]
                .spacing(SPACE_XS)
                .align_y(Alignment::Center),
            ),
        )
    }

    fn calibration_card(&self) -> Element<'_, Message> {
        let (cx, cy) = self.settings.calibration;
        let calibrated = (cx - 1.0).abs() > 1e-4 || (cy - 1.0).abs() > 1e-4;
        let rule = trim_float(TestPage::new(&self.calibration_sheet()).rule, 0);

        let mut header = row![section("Printer calibration"), space().width(Fill)].spacing(SPACE_XS).align_y(Alignment::Center);
        if calibrated {
            header = header.push(tip(
                container(text(format!("×{cx:.3} · ×{cy:.3}")).size(10)).style(theme::chip_accent).padding([1, 6]),
                "Output is scaled to compensate for this printer",
            ));
        }
        let mut col = column![
            header,
            label_button(theme::RULER, |c| c.text, "Print test page").style(theme::secondary).padding([7, 12]).width(Fill).on_press(Message::PrintTest),
            labelled(
                "Test page size to measure",
                pick_list(Paper::ALL.as_slice(), Some(self.settings.calibration_paper), Message::CalibrationPaper).width(Fill).padding([6, 10]).text_size(13).style(theme::picker)
            ),
            row![
                num_field(format!("Across ({rule} mm)"), self.text(Field::MeasuredX), "mm", Field::MeasuredX),
                num_field(format!("Down ({rule} mm)"), self.text(Field::MeasuredY), "mm", Field::MeasuredY),
            ]
            .spacing(SPACE_S),
            button(text("Apply").size(13)).style(theme::secondary).padding([6, 12]).on_press(Message::ApplyCalibration),
        ]
        .spacing(SPACE_S);
        if calibrated {
            col = col.push(button(text("Reset calibration").size(12)).style(theme::link).padding(0).on_press(Message::ResetCalibration));
        }
        card(col)
    }

    fn workspace(&self) -> Element<'_, Message> {
        let items: &[Option<usize>] = self.pages.get(self.page).map_or(&[], Vec::as_slice);
        let view = Viewport { sheet: self.sheet(), cells: &self.cells, zoom: self.zoom, pan: self.pan };
        let preview = canvas(Preview { view, images: &self.images, badges: &self.badges, items, cache: &self.cache }).width(Fill).height(Fill);
        let guides = canvas(Guides { view, items, cache: &self.guide_cache, hover: self.hover, dragging: self.dragging }).width(Fill).height(Fill);
        // iced draws a layer's images above its vector shapes, so the guides need their own
        // layer: a clipping container starts one.
        let preview = Element::from(stack![preview, container(guides).clip(true).width(Fill).height(Fill)]).map(Message::Preview);

        let pages = self.pages.len();
        let mut bar = Row::new().spacing(SPACE_S).align_y(Alignment::Center).padding([SPACE_S, SPACE_M]).push(text(self.status()).size(12).style(muted)).push(space().width(Fill));
        if pages > 1 {
            bar = bar.push(
                row![
                    icon_button(theme::CHEVRON_L, 14.0, 4, (self.page > 0).then_some(Message::Page(-1))),
                    text(format!("{} / {pages}", self.page + 1)).size(12),
                    icon_button(theme::CHEVRON_R, 14.0, 4, (self.page + 1 < pages).then_some(Message::Page(1))),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            );
        }
        let bar = bar
            .push(tip(icon_button(theme::FIT, 15.0, 6, Some(Message::ZoomFit)), "Fit page (⌘0)"))
            .push(tip(label_button(theme::EXPORT, |c| c.text, "Export PDF").style(theme::secondary).padding([7, 12]).on_press(Message::Export), "Save a PDF (⌘E)"))
            .push(tip(
                label_button(theme::PRINT, |c| c.on_accent, "Print").style(theme::primary).padding([7, 14]).on_press(Message::Print),
                "Print at 100% through the system dialog (⌘P)",
            ));

        let mut layers = stack![preview];
        if let Some(t) = &self.toast {
            layers = layers.push(container(toast(t)).width(Fill).align_x(Alignment::Center).padding(Padding::ZERO.top(SPACE_M)));
        }
        container(column![container(layers).width(Fill).height(Fill), container(bar).style(theme::sidebar).width(Fill)])
            .style(theme::workspace)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Bottom-bar summary, e.g. "2 pages · 40 badges · 3 empty · 310 dpi".
    fn status(&self) -> String {
        let per_sheet = self.cells.len();
        if self.images.is_empty() {
            return format!("{per_sheet} per sheet");
        }
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let pages = self.pages.len();
        let filled = self.pages.iter().flatten().flatten().count();
        let mut status = format!("{pages} page{} · {filled} badge{}", plural(pages), plural(filled));
        let empty = per_sheet * pages - filled;
        if empty > 0 {
            status += &format!(" · {empty} empty");
        }
        let dpi = self.images.iter().map(|i| self.sheet().effective_dpi(i.width(), i.height(), i.zoom)).fold(f32::INFINITY, f32::min);
        status + &format!(" · {} dpi", dpi.round())
    }
}

fn toast(t: &Toast) -> Element<'_, Message> {
    let mut r = Row::new()
        .spacing(SPACE_S)
        .align_y(Alignment::Center)
        .push(text(&t.text).size(13).style(move |th: &Theme| tone(th, if t.error { |c| c.danger } else { |c| c.text })));
    if let Some(p) = &t.path {
        let label = if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Show file" };
        r = r.push(button(text(label).size(13)).style(theme::link).padding(0).on_press(Message::Reveal(p.clone())));
    }
    r = r.push(icon_button(theme::CLOSE, 12.0, 3, Some(Message::DismissToast)));
    container(r).style(theme::floating).padding([SPACE_S, SPACE_M]).into()
}

// ---- small helpers ----

fn tone(theme: &Theme, pick: fn(&Colors) -> Color) -> text::Style {
    text::Style { color: Some(pick(&theme::colors(theme))) }
}

fn muted(theme: &Theme) -> text::Style {
    tone(theme, |c| c.muted)
}

fn faint(theme: &Theme) -> text::Style {
    tone(theme, |c| c.faint)
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).style(theme::card).padding([SPACE_M, 14.0]).width(Fill).into()
}

fn section(title: &str) -> Element<'_, Message> {
    text(title.to_uppercase()).size(11).font(SEMIBOLD).style(muted).into()
}

fn labelled<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(12).style(muted), control.into()].spacing(4).into()
}

fn tip<'a>(content: impl Into<Element<'a, Message>>, label: impl Into<String>) -> Element<'a, Message> {
    tooltip(content, container(text(label.into()).size(12)).style(theme::floating).padding([4, 8]), tooltip::Position::Top).gap(6).into()
}

/// A borderless, muted icon button; `None` disables it.
fn icon_button<'a>(body: &'static str, size: f32, padding: u16, on_press: Option<Message>) -> Element<'a, Message> {
    button(icon(body, size, |c| c.muted)).style(theme::ghost).padding(padding).on_press_maybe(on_press).into()
}

/// Icon + label content for a text button; the caller picks style, padding and action.
fn label_button<'a>(body: &'static str, icon_tone: fn(&Colors) -> Color, label: &'a str) -> Button<'a, Message> {
    button(row![icon(body, 14.0, icon_tone), text(label).size(13)].spacing(SPACE_XS).align_y(Alignment::Center))
}

fn switch<'a>(label: &'a str, value: bool, which: Toggle) -> Element<'a, Message> {
    toggler(value).label(label).on_toggle(move |v| Message::Toggle(which, v)).text_size(13).size(18).style(theme::switch).into()
}

fn segmented<'a, T: Copy + PartialEq + 'a>(options: &[(T, &'a str)], selected: T, msg: fn(T) -> Message) -> Element<'a, Message> {
    let buttons = options.iter().fold(Row::new().spacing(2), |r, &(value, label)| {
        r.push(button(text(label).size(12).align_x(Alignment::Center).width(Fill)).style(theme::segment(value == selected)).padding([4, 8]).width(Fill).on_press(msg(value)))
    });
    container(buttons).style(theme::segment_track).padding(2).width(Fill).into()
}

/// Numeric text input with increment/decrement arrows. Up/Down keys step it while focused.
fn stepper<'a>(value: &'a str, field: Field, width: f32, suffix: Option<&'a str>) -> Element<'a, Message> {
    let arrow = |body, dir| button(icon(body, 10.0, |c| c.muted)).style(theme::ghost).padding([0, 2]).on_press(Message::Step(field, dir));
    let input = text_input("", value).id(field.id()).on_input(move |s| Message::Text(field, s)).width(width).padding([5, 8]).size(13).style(theme::field);
    let r = row![input, column![arrow(theme::CHEVRON_UP, 1.0), arrow(theme::CHEVRON_DOWN, -1.0)]].spacing(2).align_y(Alignment::Center);
    match suffix {
        Some(suffix) => r.push(text(suffix).size(11).style(muted)).into(),
        None => r.into(),
    }
}

fn num_field<'a>(label: impl Into<String>, value: &'a str, suffix: &'a str, field: Field) -> Element<'a, Message> {
    column![text(label.into()).size(12).style(muted), stepper(value, field, 74.0, Some(suffix))].spacing(4).width(Fill).into()
}

fn slider_field<'a>(label: &'a str, field: Field, value_mm: f32, range: std::ops::RangeInclusive<f32>, text_value: &'a str, units: Units) -> Element<'a, Message> {
    column![
        row![text(label).size(12).style(muted), space().width(Fill), stepper(text_value, field, 56.0, Some(units.suffix()))].spacing(SPACE_XS).align_y(Alignment::Center),
        slider(range, value_mm, move |v| Message::Slide(field, v)).step(0.5_f32).style(theme::slider_style),
    ]
    .spacing(4)
    .into()
}

const SWATCHES: &[[f32; 3]] = &[
    [0.96, 0.91, 0.83], // cream
    [0.99, 0.90, 0.54], // yellow
    [0.99, 0.65, 0.65], // coral
    [0.98, 0.66, 0.83], // pink
    [0.77, 0.71, 0.99], // violet
    [0.58, 0.77, 0.99], // sky
    [0.53, 0.94, 0.67], // mint
    [0.84, 0.83, 0.82], // stone
    [0.12, 0.16, 0.22], // charcoal
    [0.0, 0.0, 0.0],    // black
];

/// Shorten a name to about `max` characters, keeping its start and end ("long-file…name.png").
fn truncate_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let keep_end = (max / 3).max(6);
    let keep_start = max.saturating_sub(keep_end + 1);
    let start: String = s.chars().take(keep_start).collect();
    let end: String = s.chars().skip(n - keep_end).collect();
    format!("{start}…{end}")
}
