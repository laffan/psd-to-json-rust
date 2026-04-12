use anyhow::Result;
use image::{RgbaImage, RgbImage};
use serde_json::{Map, Value};

use super::sprite::{layer_to_cropped_image, export_mask};
use crate::config::Config;
use crate::export::image_export;
use rayon::prelude::*;
use std::path::Path;

/// Process a tileset layer: slice the layer image into a grid of tiles,
/// optionally creating scaled versions.
pub fn process_tiles(
    layer: &psd::PsdLayer,
    layer_info: &Map<String, Value>,
    config: &Config,
    output_dir: &Path,
    psd: &psd::Psd,
) -> Result<Map<String, Value>> {
    let mut result = layer_info.clone();

    let name = layer_info.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");
    let tile_type = layer_info.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let is_jpg = tile_type.eq_ignore_ascii_case("jpg");

    let lw = layer.width() as u32;
    let lh = layer.height() as u32;

    let tile_size = config.tile_slice_size;
    let num_tiles_x = (lw + tile_size - 1) / tile_size;
    let num_tiles_y = (lh + tile_size - 1) / tile_size;

    result.insert("columns".into(), Value::from(num_tiles_x));
    result.insert("rows".into(), Value::from(num_tiles_y));
    result.insert("filetype".into(), Value::String(if is_jpg { "jpg" } else { "png" }.into()));

    // Export mask
    export_mask(layer, &mut result, output_dir, "masks", name, config.metadata_only)?;

    if config.metadata_only {
        return Ok(result);
    }

    // Get the composited layer image
    let tile_image = match layer_to_cropped_image(layer, psd.width(), psd.height()) {
        Some(img) => img,
        None => return Ok(result),
    };

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

        let tile = image::imageops::crop_imm(&tile_image, left, top_coord, tw, th).to_image();

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
