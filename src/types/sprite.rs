use anyhow::Result;
use image::RgbaImage;
use serde_json::{Map, Value};
use std::path::Path;

use super::sprite_animation::AnimationSprite;
use super::sprite_atlas::AtlasSprite;
use super::sprite_basic::BasicSprite;
use super::sprite_sheet::SpritesheetSprite;
use crate::export::image_export;

/// Common sprite context shared across sprite sub-types.
pub struct SpriteContext<'a> {
    pub layer_info: &'a Map<String, Value>,
    pub config: &'a crate::config::Config,
    pub sprite_output_dir: &'a Path,
    /// When the sprite is a PSD group folder, this is its group ID.
    pub group_id: Option<u32>,
}

/// Trait that all sprite sub-types implement.
pub trait SpriteProcessor {
    fn process(&self, ctx: &SpriteContext, layers: &[psd::PsdLayer], psd: &psd::Psd)
        -> Result<Map<String, Value>>;
}

/// Factory: create the right sprite processor based on the type field.
pub fn create_sprite_processor(layer_type: Option<&str>) -> Box<dyn SpriteProcessor> {
    match layer_type {
        Some("spritesheet") => Box::new(SpritesheetSprite),
        Some("animation") => Box::new(AnimationSprite),
        Some("atlas") => Box::new(AtlasSprite),
        _ => Box::new(BasicSprite),
    }
}

/// Export a layer's raster mask to a PNG file if it exists.
/// Modifies `result` in place with mask metadata.
///
/// Masks that are entirely white (255) are "reveal all" no-ops and are skipped
/// to match the Python psd-to-json behaviour.
pub fn export_mask(
    layer: &psd::PsdLayer,
    result: &mut Map<String, Value>,
    output_dir: &Path,
    subdir: &str,
    name: &str,
    metadata_only: bool,
) -> Result<()> {
    let mask = match layer.mask() {
        Some(m) => m,
        None => return Ok(()),
    };

    // Skip "reveal all" masks (all-white / 255 pixels) — they have no visual
    // effect and the Python version doesn't include them. Decompressed once
    // and kept: the export below reads the same pixels.
    let mask_pixels = layer.mask_pixels();
    if let Some(mask_data) = &mask_pixels {
        if mask_data.iter().all(|&b| b == 255) {
            return Ok(());
        }
    }

    result.insert("mask".into(), Value::Bool(true));
    result.insert("maskX".into(), Value::from(mask.left));
    result.insert("maskY".into(), Value::from(mask.top));
    result.insert("maskWidth".into(), Value::from(mask.width()));
    result.insert("maskHeight".into(), Value::from(mask.height()));

    if metadata_only {
        return Ok(());
    }

    let mask_dir = output_dir.join(subdir);
    std::fs::create_dir_all(&mask_dir)?;

    // Get mask pixel data from the layer
    if let Some(mask_data) = mask_pixels {
        let w = mask.width();
        let h = mask.height();
        if w > 0 && h > 0 {
            // mask_pixels returns a grayscale buffer; convert to RGBA
            let mut rgba = RgbaImage::new(w, h);
            for (i, &gray) in mask_data.iter().enumerate() {
                let px = (i as u32) % w;
                let py = (i as u32) / w;
                if px < w && py < h {
                    rgba.put_pixel(px, py, image::Rgba([gray, gray, gray, 255]));
                }
            }
            let mask_filename = format!("{}_mask.png", name);
            let mask_path = mask_dir.join(&mask_filename);
            image_export::save_png(&rgba, &mask_path)?;

            let rel_path = format!("{}/{}", subdir, mask_filename);
            result.insert("maskPath".into(), Value::String(rel_path));
        }
    }

    Ok(())
}

/// Extract RGBA pixel data for a single layer, cropped to its actual bounds.
/// Returns the image sized to the layer's own dimensions (not the full PSD canvas).
pub fn layer_to_cropped_image(layer: &psd::PsdLayer, psd_width: u32, psd_height: u32) -> Option<RgbaImage> {
    let lw = layer.width() as u32;
    let lh = layer.height() as u32;
    if lw == 0 || lh == 0 {
        return None;
    }

    let rgba_data = layer.composite_rgba();
    let canvas_w = psd_width as usize;
    let canvas_h = psd_height as usize;

    // A row at a time rather than a pixel at a time: the layer's window onto
    // the canvas buffer is a run of whole rows, and at print resolution a
    // sprite is millions of pixels. The window starts at the layer's corner
    // clamped to the canvas, and whatever of the layer falls past the
    // canvas's right or bottom edge stays clear.
    let left = layer.layer_left().max(0) as usize;
    let top = layer.layer_top().max(0) as usize;
    let mut img = RgbaImage::new(lw, lh);
    let run = (lw as usize).min(canvas_w.saturating_sub(left));
    if run > 0 {
        let out: &mut [u8] = &mut img;
        for dy in 0..(lh as usize).min(canvas_h.saturating_sub(top)) {
            let from = ((top + dy) * canvas_w + left) * 4;
            let Some(source) = rgba_data.get(from..from + run * 4) else {
                break;
            };
            let into = dy * lw as usize * 4;
            out[into..into + run * 4].copy_from_slice(source);
        }
    }

    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use psd::{LayerBuilder, Psd, PsdBuilder};

    /// The crop as it was written before it copied whole rows: one pixel at
    /// a time, through `put_pixel`.
    fn pixel_by_pixel(layer: &psd::PsdLayer, canvas_w: u32, canvas_h: u32) -> RgbaImage {
        let (lw, lh) = (layer.width() as u32, layer.height() as u32);
        let rgba = layer.composite_rgba();
        let mut img = RgbaImage::new(lw, lh);
        let left = layer.layer_left().max(0) as u32;
        let top = layer.layer_top().max(0) as u32;
        for dy in 0..lh {
            for dx in 0..lw {
                let (sx, sy) = (left + dx, top + dy);
                if sx < canvas_w && sy < canvas_h {
                    let i = ((sy * canvas_w + sx) * 4) as usize;
                    img.put_pixel(dx, dy, image::Rgba([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]));
                }
            }
        }
        img
    }

    /// Every pixel different, so a row landing one place off shows.
    fn gradient(w: u32, h: u32) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| [(i % 251) as u8, (i / 7 % 253) as u8, (i % 13 * 19) as u8, 200])
            .collect()
    }

    #[test]
    fn whole_rows_crop_what_single_pixels_did() {
        let (cw, ch) = (40, 30);
        let placed = [(5, 4, 12, 9), (-6, 3, 15, 8), (30, 25, 20, 12), (-3, -2, 50, 40), (0, 0, 40, 30)];
        let mut builder = PsdBuilder::new(cw, ch);
        for (i, (x, y, w, h)) in placed.iter().enumerate() {
            builder.add_layer(
                LayerBuilder::new(format!("S | l{i}"))
                    .rgba(*w, *h, gradient(*w, *h))
                    .at(*x, *y)
                    .opacity(if i == 2 { 128 } else { 255 }),
            );
        }
        let doc = Psd::from_bytes(&builder.to_bytes().unwrap()).unwrap();
        for layer in doc.layers() {
            let rows = layer_to_cropped_image(layer, cw, ch).unwrap();
            assert_eq!(rows, pixel_by_pixel(layer, cw, ch), "{}", layer.name());
        }
    }
}
