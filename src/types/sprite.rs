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
    // effect and the Python version doesn't include them.
    if let Some(mask_data) = layer.mask_pixels() {
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
    if let Some(mask_data) = layer.mask_pixels() {
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
    let canvas_w = psd_width;
    let canvas_h = psd_height;

    let mut img = RgbaImage::new(lw, lh);
    let left = layer.layer_left().max(0) as u32;
    let top = layer.layer_top().max(0) as u32;

    for dy in 0..lh {
        for dx in 0..lw {
            let sx = left + dx;
            let sy = top + dy;
            if sx < canvas_w && sy < canvas_h {
                let idx = ((sy * canvas_w + sx) * 4) as usize;
                if idx + 3 < rgba_data.len() {
                    img.put_pixel(
                        dx,
                        dy,
                        image::Rgba([
                            rgba_data[idx],
                            rgba_data[idx + 1],
                            rgba_data[idx + 2],
                            rgba_data[idx + 3],
                        ]),
                    );
                }
            }
        }
    }

    Some(img)
}
