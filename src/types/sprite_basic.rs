use anyhow::Result;
use image::RgbaImage;
use serde_json::{Map, Value};

use super::sprite::{export_mask, layer_to_cropped_image, SpriteContext, SpriteProcessor};
use crate::export::image_export;

pub struct BasicSprite;

impl SpriteProcessor for BasicSprite {
    fn process(
        &self,
        ctx: &SpriteContext,
        layers: &[psd::PsdLayer],
        psd: &psd::Psd,
    ) -> Result<Map<String, Value>> {
        let mut result = ctx.layer_info.clone();
        let name = ctx.layer_info.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");

        // Check if this sprite corresponds to a group (has children layers)
        // We determine this by checking if any layer has this as a parent
        let layer_name_full = find_layer_by_parsed_name(layers, name);
        let is_group = layer_name_full
            .map(|l| {
                // Check if this layer is part of a group via parent_id matching
                let groups = psd.groups();
                groups.values().any(|g| g.name() == l.name())
            })
            .unwrap_or(false);

        if is_group {
            // Find the group and merge its children
            if let Some(group_layer) = layer_name_full {
                let group_id = psd.groups().iter().find(|(_, g)| g.name() == group_layer.name()).map(|(&id, _)| id);
                if let Some(gid) = group_id {
                    let _group = &psd.groups()[&gid];
                    let lw = result.get("width").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                    let lh = result.get("height").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                    let lx = result.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let ly = result.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

                    if !ctx.config.metadata_only && lw > 0 && lh > 0 {
                        let mut merged = RgbaImage::new(lw, lh);

                        // Composite child layers
                        if let Some(sub_layers) = psd.get_group_sub_layers(&gid) {
                            for child in sub_layers.iter() {
                                if !child.visible() {
                                    continue;
                                }
                                // Only include direct children (whose parent == this group)
                                if child.parent_id() != Some(gid) {
                                    continue;
                                }
                                if let Some(child_img) = layer_to_cropped_image(child, psd.width(), psd.height()) {
                                    let cx = child.layer_left() - lx;
                                    let cy = child.layer_top() - ly;
                                    alpha_composite(&mut merged, &child_img, cx, cy);
                                }
                            }
                        }

                        let filename = format!("{}.png", name);
                        let filepath = ctx.sprite_output_dir.join(&filename);
                        std::fs::create_dir_all(ctx.sprite_output_dir)?;
                        image_export::save_png(&merged, &filepath)?;
                        result.insert("filePath".into(), Value::String(format!("sprites/{}", filename)));
                    }

                    // Export mask from the group layer if present
                    if let Some(layer) = layer_name_full {
                        export_mask(layer, &mut result, ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir), "sprites", name, ctx.config.metadata_only)?;
                    }
                }
            }
        } else {
            // Single layer sprite
            if let Some(layer) = layer_name_full {
                let lw = layer.width() as u32;
                let lh = layer.height() as u32;
                result.insert("width".into(), Value::from(lw));
                result.insert("height".into(), Value::from(lh));

                export_mask(layer, &mut result, ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir), "sprites", name, ctx.config.metadata_only)?;

                if !ctx.config.metadata_only {
                    if let Some(img) = layer_to_cropped_image(layer, psd.width(), psd.height()) {
                        let filename = format!("{}.png", name);
                        let filepath = ctx.sprite_output_dir.join(&filename);
                        std::fs::create_dir_all(ctx.sprite_output_dir)?;
                        image_export::save_png(&img, &filepath)?;
                        result.insert("filePath".into(), Value::String(format!("sprites/{}", filename)));
                    }
                }
            }
        }

        Ok(result)
    }
}

fn find_layer_by_parsed_name<'a>(layers: &'a [psd::PsdLayer], parsed_name: &str) -> Option<&'a psd::PsdLayer> {
    use crate::parser::parse_layer_name;
    layers.iter().find(|l| {
        parse_layer_name(l.name())
            .map(|p| p.name == parsed_name)
            .unwrap_or(false)
    })
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
