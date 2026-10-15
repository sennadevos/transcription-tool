//! Company marks next to model names. The images are white-on-transparent masks tinted with the text colour, so they suit light and dark.
//! Companies without a mark get a neutral generic one.

use std::io::Cursor;

use eframe::egui::{self, Color32, ColorImage, Id, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2};

/// (file name, model-id prefixes that belong to it, image)
const MARKS: &[(&str, &[&str], &[u8])] = &[
    ("anthropic", &["anthropic"], include_bytes!("../assets/icons/anthropic.png")),
    ("openai", &["openai"], include_bytes!("../assets/icons/openai.png")),
    ("google", &["google"], include_bytes!("../assets/icons/google.png")),
    ("meta", &["meta", "meta-llama"], include_bytes!("../assets/icons/meta.png")),
    ("mistral", &["mistralai"], include_bytes!("../assets/icons/mistral.png")),
    ("xai", &["x-ai"], include_bytes!("../assets/icons/xai.png")),
    ("nvidia", &["nvidia"], include_bytes!("../assets/icons/nvidia.png")),
    ("deepseek", &["deepseek"], include_bytes!("../assets/icons/deepseek.png")),
    ("qwen", &["qwen"], include_bytes!("../assets/icons/qwen.png")),
    ("elevenlabs", &["elevenlabs"], include_bytes!("../assets/icons/elevenlabs.png")),
];

/// The company part of a model id: `anthropic/claude-haiku-5.5` and `~anthropic/claude-haiku-latest` both give `anthropic`.
fn company(model_id: &str) -> &str {
    model_id.trim_start_matches('~').split('/').next().unwrap_or_default()
}

fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &buf[..info.buffer_size()]))
}

fn texture(ctx: &egui::Context, name: &'static str, bytes: &[u8]) -> Option<TextureHandle> {
    let id = Id::new(("company-mark", name));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let t = ctx.load_texture(name, decode(bytes)?, TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear)));
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// Draws the mark of the company behind `model_id` in a square of `size` points.
pub fn show(ui: &mut egui::Ui, model_id: &str, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let who = company(model_id);
    if who == "microsoft" {
        // Four squares with a visible gap; as a scaled-down image the gaps blur into one block.
        let gap = rect.width() * 0.12;
        let side = (rect.width() * 0.84 - gap) / 2.0;
        let origin = rect.min + Vec2::splat(rect.width() * 0.08);
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let min = origin + Vec2::new(dx, dy) * (side + gap);
            ui.painter().rect_filled(Rect::from_min_size(min, Vec2::splat(side)), 0.0, color);
        }
        return;
    }
    let mark = MARKS.iter().find(|(_, prefixes, _)| prefixes.contains(&who)).and_then(|(name, _, bytes)| texture(ui.ctx(), name, bytes));
    match mark {
        Some(tex) => {
            ui.painter().image(tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), color);
        }
        None => generic(ui.painter(), rect, color),
    }
}

/// A plain outlined square with a dot: says "some model" without pretending to be a brand.
fn generic(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.1);
    painter.rect_stroke(r, 3.0, Stroke::new(1.3, color.gamma_multiply(0.6)), StrokeKind::Inside);
    painter.circle_filled(r.center(), r.width() * 0.13, color.gamma_multiply(0.6));
}
