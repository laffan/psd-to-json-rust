use anyhow::Result;
use image::{RgbaImage, RgbImage};
use serde_json::{Map, Value};

use super::sprite::layer_to_cropped_image;
use crate::config::Config;
use crate::export::image_export;
use rayon::prelude::*;
use std::path::Path;

/// Process a tileset: slice an image into a grid of tiles, optionally
/// creating scaled versions.
///
/// `tile_image` is the fully-composited RGBA image to slice.
/// If `mask_layer` is provided, its raster mask is exported too.
pub fn process_tiles(
    tile_image: &RgbaImage,
    layer_info: &Map<String, Value>,
    config: &Config,
    output_dir: &Path,
    mask_layer: Option<&psd::PsdLayer>,
) -> Result<Map<String, Value>> {
    let mut result = layer_info.clone();

    let name = layer_info.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");
    let tile_type = layer_info.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let is_jpg = tile_type.eq_ignore_ascii_case("jpg");

    let (lw, lh) = tile_image.dimensions();

    let tile_size = config.tile_slice_size;
    let num_tiles_x = (lw + tile_size - 1) / tile_size;
    let num_tiles_y = (lh + tile_size - 1) / tile_size;

    result.insert("columns".into(), Value::from(num_tiles_x));
    result.insert("rows".into(), Value::from(num_tiles_y));
    result.insert("filetype".into(), Value::String(if is_jpg { "jpg" } else { "png" }.into()));

    // Export mask if a source layer was provided
    if let Some(layer) = mask_layer {
        super::sprite::export_mask(layer, &mut result, output_dir, "masks", name, config.metadata_only)?;
    }

    if config.metadata_only {
        return Ok(result);
    }

    let tiles_base_dir = output_dir.join("tiles").join(name);
    let tiles_dir = tiles_base_dir.join(tile_size.to_string());
    std::fs::create_dir_all(&tiles_dir)?;

    println!("Slicing {} into {}px tiles ({} x {})...", name, tile_size, num_tiles_x, num_tiles_y);

    // Generate tile coordinates
    let tile_coords: Vec<(u32, u32)> = (0..num_tiles_y)
        .flat_map(|y| (0..num_tiles_x).map(move |x| (x, y)))
        .collect();

    // Slice and save tiles in parallel
    tile_coords.par_iter().try_for_each(|&(tx, ty)| -> Result<()> {
        let left = tx * tile_size;
        let top_coord = ty * tile_size;
        let right = (left + tile_size).min(lw);
        let bottom = (top_coord + tile_size).min(lh);
        let tw = right - left;
        let th = bottom - top_coord;

        if tw == 0 || th == 0 {
            return Ok(());
        }

        let tile = image::imageops::crop_imm(tile_image, left, top_coord, tw, th).to_image();

        let tile_filename = if is_jpg {
            format!("{}_tile_{}_{}.jpg", name, tx, ty)
        } else {
            format!("{}_tile_{}_{}.png", name, tx, ty)
        };
        let tile_path = tiles_dir.join(&tile_filename);

        if is_jpg {
            let rgb = rgba_to_rgb(&tile);
            image_export::save_jpg(&rgb, &tile_path, config.jpg_quality)?;
        } else {
            image_export::save_png(&tile, &tile_path)?;
        }

        Ok(())
    })?;

    // Create scaled versions
    for &scaled_size in &config.tile_scaled_versions {
        if scaled_size == tile_size {
            continue;
        }
        let scaled_dir = tiles_base_dir.join(scaled_size.to_string());
        std::fs::create_dir_all(&scaled_dir)?;

        println!("Creating scaled version at {}px...", scaled_size);

        tile_coords.par_iter().try_for_each(|&(tx, ty)| -> Result<()> {
            let ext = if is_jpg { "jpg" } else { "png" };
            let tile_filename = format!("{}_tile_{}_{}.{}", name, tx, ty, ext);
            let source_path = tiles_dir.join(&tile_filename);
            let scaled_path = scaled_dir.join(&tile_filename);

            let tile_img = image::open(&source_path)?;
            let scaled = tile_img.resize(scaled_size, scaled_size, image::imageops::FilterType::Lanczos3);

            if is_jpg {
                let rgb = scaled.to_rgb8();
                image_export::save_jpg(&rgb, &scaled_path, config.jpg_quality)?;
            } else {
                let rgba = scaled.to_rgba8();
                image_export::save_png(&rgba, &scaled_path)?;
            }

            Ok(())
        })?;
    }

    println!("Finished processing tiles for {}", name);
    Ok(result)
}

fn rgba_to_rgb(rgba: &RgbaImage) -> RgbImage {
    let (w, h) = rgba.dimensions();
    let mut rgb = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let px = rgba.get_pixel(x, y);
            rgb.put_pixel(x, y, image::Rgb([px[0], px[1], px[2]]));
        }
    }
    rgb
}

/// Composite all visible direct children of a group into a single RGBA image.
/// Returns the image and its top-left origin in PSD coordinates.
pub fn composite_group(
    gid: u32,
    psd: &psd::Psd,
) -> Option<(RgbaImage, i32, i32)> {
    let (top, left, bottom, right) = psd.group_bounds(gid)?;
    let w = ((right - left) + 1).max(0) as u32;
    let h = ((bottom - top) + 1).max(0) as u32;
    if w == 0 || h == 0 {
        return None;
    }

    let mut merged = RgbaImage::new(w, h);
    let psd_w = psd.width();
    let psd_h = psd.height();

    if let Some(sub_layers) = psd.get_group_sub_layers(&gid) {
        // Iterate in reverse: sub_layers is ordered top-to-bottom (front-to-back),
        // but we need to composite bottom-to-top (back-to-front) so that upper
        // layers are painted on top of lower ones.
        for child in sub_layers.iter().rev() {
            if !child.visible() {
                continue;
            }
            // Only direct children — skip layers nested in sub-groups.
            if child.parent_id() != Some(gid) {
                continue;
            }
            if let Some(child_img) = layer_to_cropped_image(child, psd_w, psd_h) {
                let cx = child.layer_left() - left;
                let cy = child.layer_top() - top;
                alpha_composite(&mut merged, &child_img, cx, cy);
            }
        }
    }

    Some((merged, left, top))
}

/// Alpha-composite `src` onto `dst` at position (dx, dy).
fn alpha_composite(dst: &mut RgbaImage, src: &RgbaImage, dx: i32, dy: i32) {
    let (sw, sh) = src.dimensions();
    for sy in 0..sh {
        for sx in 0..sw {
            let tx = dx + sx as i32;
            let ty = dy + sy as i32;
            if tx < 0 || ty < 0 {
                continue;
            }
            let tx = tx as u32;
            let ty = ty as u32;
            if tx >= dst.width() || ty >= dst.height() {
                continue;
            }

            let src_px = src.get_pixel(sx, sy);
            if src_px[3] == 0 {
                continue;
            }

            let dst_px = dst.get_pixel(tx, ty);
            let sa = src_px[3] as f32 / 255.0;
            let da = dst_px[3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);

            if out_a == 0.0 {
                continue;
            }

            let r = ((src_px[0] as f32 * sa + dst_px[0] as f32 * da * (1.0 - sa)) / out_a) as u8;
            let g = ((src_px[1] as f32 * sa + dst_px[1] as f32 * da * (1.0 - sa)) / out_a) as u8;
            let b = ((src_px[2] as f32 * sa + dst_px[2] as f32 * da * (1.0 - sa)) / out_a) as u8;
            let a = (out_a * 255.0) as u8;

            dst.put_pixel(tx, ty, image::Rgba([r, g, b, a]));
        }
    }
}
