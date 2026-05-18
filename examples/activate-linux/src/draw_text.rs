use std::collections::HashMap;

use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
use swash::zeno::Vector;


const FONT_DATA: &[u8] = include_bytes!("../segoeuil.ttf");

const UPPER_TEXT: &str = "Activate Linux";
const UPPER_SIZE_PT: f32 = 28.0;
const UPPER_ADD_SPACING: f32 = 0.8;

const LOWER_TEXT: &str = "Go to Settings to activate Linux.";
const LOWER_SIZE_PT: f32 = 21.0;
const LOWER_ADD_SPACING: f32 = 0.4;

const BASELINE_SPACING: f32 = 38.0;

pub(crate) const BOTTOM_MARGIN: i32 = 100;
pub(crate) const RIGHT_MARGIN: i32 = 150;


#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: HashMap<(u32, u32), u8>,
}
impl Image {
    pub fn max_y_coordinate(&self) -> u32 {
        self.pixels
            .keys()
            .map(|(_x, y)| *y)
            .max()
            .unwrap_or(0)
    }

    pub fn to_white_argb_le(&self) -> Vec<u8> {
        let pixel_color_count: usize = (self.width * self.height * 4).try_into().unwrap();
        let mut pixels = Vec::with_capacity(pixel_color_count);
        for y in 0..self.height {
            for x in 0..self.width {
                let alpha_value = self.pixels.get(&(x, y))
                    .copied()
                    .unwrap_or(0x00);
                let argb =
                    ((alpha_value as u32) << 24)
                    | ((alpha_value as u32) << 16)
                    | ((alpha_value as u32) <<  8)
                    | ((alpha_value as u32) <<  0);
                pixels.extend_from_slice(&argb.to_le_bytes());
            }
        }
        pixels
    }
}


fn draw_string(font: FontRef<'_>, text: &str, size: f32, add_spacing: f32) -> Image {
    // load font
    let metrics = font.metrics(&[]);
    let ascender_px_f32 = metrics.ascent * size / f32::from(metrics.units_per_em);
    let ascender_px: i32 = ascender_px_f32.ceil() as i32;

    // shape text
    let mut shape_ctx = ShapeContext::new();
    let mut shaper = shape_ctx.builder(font)
        .size(size)
        .build();
    shaper.add_str(text);
    let mut glyphs = Vec::new();
    shaper.shape_with(|cluster| {
        for glyph in cluster.glyphs {
            glyphs.push(*glyph);
        }
    });

    // render text
    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font)
        .size(size)
        .hint(false)
        .build();
    let mut renderer = Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ]);
    let mut pixel_values: HashMap<(u32, u32), u8> = HashMap::new();
    let mut pos_x: f32 = 0.0;
    for glyph in &glyphs {
        let pos_x_int: u32 = pos_x.trunc() as u32;
        let pos_x_frac = pos_x.fract();
        renderer.offset(Vector::new(pos_x_frac, 0.0));
        let img = renderer.render(&mut scaler, glyph.id)
            .expect("failed to render glyph");

        for y in 0..img.placement.height {
            for x in 0..img.placement.width {
                let i: usize = (y * img.placement.width + x).try_into().unwrap();
                let b = img.data[i];
                if b == 0 {
                    continue;
                }
                let actual_x: u32 = match (img.placement.left + i32::try_from(pos_x_int + x).unwrap()).try_into() {
                    Ok(ax) => ax,
                    Err(_) => continue,
                };
                let actual_y: u32 = match (ascender_px - img.placement.top + i32::try_from(y).unwrap()).try_into() {
                    Ok(ay) => ay,
                    Err(_) => continue,
                };
                let pixel_ref = pixel_values
                    .entry((actual_x, actual_y))
                    .or_insert(0);
                *pixel_ref = pixel_ref.saturating_add(b);
            }
        }

        pos_x += glyph.advance + add_spacing;
    }

    let final_width = pos_x.ceil() as u32;
    let mut ret = Image {
        width: final_width,
        height: 0,
        pixels: pixel_values,
    };
    ret.height = ret.max_y_coordinate();
    ret
}


pub fn draw_text(scale: f32) -> Image {
    let font = FontRef::from_index(FONT_DATA, 0)
        .expect("failed to load font");

    let upper_image = draw_string(
        font,
        UPPER_TEXT,
        UPPER_SIZE_PT * scale,
        UPPER_ADD_SPACING,
    );
    let lower_image = draw_string(
        font,
        LOWER_TEXT,
        LOWER_SIZE_PT * scale,
        LOWER_ADD_SPACING,
    );

    let scaled_baseline_spacing: u32 = (BASELINE_SPACING * scale).round() as u32;

    let mut all_text_image = Image {
        width: 0,
        height: 0,
        pixels: HashMap::new(),
    };
    for ((x, y), value) in upper_image.pixels {
        let pixel_ref = all_text_image.pixels
            .entry((x, y))
            .or_insert(0);
        *pixel_ref = pixel_ref.saturating_add(value);
    }
    for ((x, y), value) in lower_image.pixels {
        let pixel_ref = all_text_image.pixels
            .entry((x, y + scaled_baseline_spacing))
            .or_insert(0);
        *pixel_ref = pixel_ref.saturating_add(value);
    }
    all_text_image.width = upper_image.width.max(lower_image.width);
    all_text_image.height = all_text_image.max_y_coordinate();
    all_text_image
}
