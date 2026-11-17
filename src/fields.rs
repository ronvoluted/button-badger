//! Numeric text fields: what each one is, and how typed text reaches the model and back.

use crate::App;
use crate::images::wrap_degrees;
use crate::layout::{Units, trim_float};

/// Every numeric text field. Each keeps its own text so half-typed values ("3.") survive.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Field {
    Face,
    Bleed,
    Gap,
    Margin,
    PaperW,
    PaperH,
    Weight,
    MeasuredX,
    MeasuredY,
    Copies(usize),
    Scale(usize),
    Rotation(usize),
}

impl Field {
    pub fn id(self) -> iced::widget::Id {
        iced::widget::Id::from(format!("field-{self:?}"))
    }

    /// Step size in the field's display unit.
    fn step(self, units: Units) -> f32 {
        match self {
            Field::Weight => 0.05,
            Field::Copies(_) => 1.0,
            Field::Scale(_) => 5.0,
            Field::Rotation(_) => 90.0,
            Field::MeasuredX | Field::MeasuredY => 0.5,
            _ => match units {
                Units::Mm => 0.5,
                Units::Inch => 0.05,
            },
        }
    }

    fn decimals(self, units: Units) -> usize {
        match self {
            Field::Copies(_) | Field::Scale(_) | Field::Rotation(_) => 0,
            Field::Weight => 2,
            _ if units == Units::Inch => 3,
            _ => 1,
        }
    }
}

impl App {
    pub fn text(&self, f: Field) -> &str {
        self.texts.get(&f).map(String::as_str).unwrap_or("")
    }

    /// Reformat every sheet field from the model (after units, presets or sliders change it).
    pub fn sync_texts(&mut self) {
        let s = self.sheet();
        let u = s.units;
        let values = [
            (Field::Face, u.format(s.face_mm)),
            (Field::Bleed, u.format(s.bleed_mm)),
            (Field::Gap, u.format(s.gap_mm)),
            (Field::Margin, u.format(s.margin_mm)),
            (Field::PaperW, u.format(s.custom_paper_mm.0)),
            (Field::PaperH, u.format(s.custom_paper_mm.1)),
            (Field::Weight, trim_float(s.guides.weight_pt, 2)),
        ];
        self.texts.extend(values);
    }

    pub fn sync_image_texts(&mut self) {
        self.texts.retain(|f, _| !matches!(f, Field::Copies(_) | Field::Scale(_) | Field::Rotation(_)));
        for (i, img) in self.images.iter().enumerate() {
            self.texts.insert(Field::Copies(i), img.qty.to_string());
            self.texts.insert(Field::Scale(i), trim_float(img.zoom * 100.0, 0));
            self.texts.insert(Field::Rotation(i), trim_float(img.rotation, 0));
        }
    }

    /// Apply typed text to the model (copies, scale, rotation or a dimension).
    pub fn apply_text(&mut self, field: Field, s: String) {
        match field {
            Field::Copies(i) => {
                let digits = clean(&s, "", 4);
                if let Some(img) = self.images.get_mut(i) {
                    img.qty = digits.parse().unwrap_or(0);
                    self.texts.insert(field, digits);
                    self.relayout();
                }
            }
            Field::Scale(i) => {
                let cleaned = clean(&s, ".", 6);
                let parsed = cleaned.parse::<f32>();
                self.texts.insert(field, cleaned);
                if let (Ok(v), Some(img)) = (parsed, self.images.get_mut(i))
                    && v.is_finite()
                    && v >= 1.0
                {
                    img.zoom = (v / 100.0).clamp(0.05, 20.0);
                    self.image_changed(i);
                }
            }
            Field::Rotation(i) => {
                let cleaned = clean(&s, ".-", 7);
                if let (Ok(v), Some(img)) = (cleaned.parse::<f32>(), self.images.get_mut(i))
                    && v.is_finite()
                {
                    img.rotation = wrap_degrees(v);
                    // Show the wrapped angle once it differs (e.g. 270 → -90), but leave "-" or "4" alone mid-typing.
                    let shown = if (img.rotation - v).abs() > 0.001 { trim_float(img.rotation, 0) } else { cleaned };
                    self.texts.insert(field, shown);
                    self.image_changed(i);
                } else {
                    self.texts.insert(field, cleaned);
                }
            }
            Field::MeasuredX | Field::MeasuredY => {
                // Only read when "Apply" is pressed.
                self.texts.insert(field, clean(&s, ".,", 8));
            }
            _ => {
                let cleaned = clean(&s, ".,", 8);
                let parsed = parse_decimal(&cleaned);
                self.texts.insert(field, cleaned);
                if let Some(v) = parsed {
                    let v = if field == Field::Weight { v } else { self.units().to_mm(v) };
                    self.set_dimension(field, v);
                    self.sheet_changed();
                }
            }
        }
    }

    /// Nudge a numeric field by `dir` steps and re-apply it.
    pub fn step_field(&mut self, field: Field, dir: f32) {
        let current = parse_decimal(self.text(field)).unwrap_or(0.0);
        let step = field.step(self.units());
        let next = (current / step).round() * step + dir * step;
        let next = if matches!(field, Field::Rotation(_)) { next } else { next.max(0.0) };
        self.apply_text(field, trim_float(next, field.decimals(self.units())));
    }

    /// Set a sheet dimension, clamped to a sane range. Lengths in mm, weight in points.
    pub fn set_dimension(&mut self, field: Field, v: f32) {
        let s = &mut self.settings.sheet;
        match field {
            Field::Face => s.face_mm = v.clamp(5.0, 300.0),
            Field::Bleed => s.bleed_mm = v.clamp(0.0, 50.0),
            Field::Gap => s.gap_mm = v.clamp(0.0, 100.0),
            Field::Margin => s.margin_mm = v.clamp(0.0, 100.0),
            Field::PaperW => s.custom_paper_mm.0 = v.clamp(20.0, 2000.0),
            Field::PaperH => s.custom_paper_mm.1 = v.clamp(20.0, 2000.0),
            Field::Weight => s.guides.weight_pt = v.clamp(0.05, 5.0),
            Field::MeasuredX | Field::MeasuredY | Field::Copies(_) | Field::Scale(_) | Field::Rotation(_) => {}
        }
    }
}

/// Parse a number typed with either decimal separator.
pub fn parse_decimal(s: &str) -> Option<f32> {
    s.replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

/// What is kept of typed text: digits and the `extra` characters, up to `max` of them.
fn clean(s: &str, extra: &str, max: usize) -> String {
    s.chars().filter(|c| c.is_ascii_digit() || extra.contains(*c)).take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_and_field_ids() {
        assert_eq!(parse_decimal("2,5"), Some(2.5));
        assert_eq!(parse_decimal("."), None);
        assert_eq!(clean("-1x2.5,6", ".-", 5), "-12.5");
        // Arrow-key stepping finds the focused field by comparing ids.
        assert_eq!(Field::Copies(2).id(), Field::Copies(2).id());
        assert_ne!(Field::Copies(2).id(), Field::Copies(1).id());
    }
}
