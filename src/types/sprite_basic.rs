use anyhow::Result;
use image::RgbaImage;
use serde_json::{Map, Value};

use super::sprite::{export_mask, layer_to_cropped_image, SpriteContext, SpriteProcessor};
use crate::export::image_export;
use crate::parser::parse_layer_name;

pub struct BasicSprite;

impl SpriteProcessor for BasicSprite {
    fn process(
        &self,
        ctx: &SpriteContext,
        layers: &[psd::PsdLayer],
        psd: &psd::Psd,
    ) -> Result<Map<String, Value>> {
        let mut result = ctx.layer_info.clone();
        let name = ctx.layer_info
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed");

        // If we already know the group ID (caller told us), use it directly.
        // Otherwise try to find this sprite as a flat pixel layer.
        if let Some(gid) = ctx.group_id {
            self.process_group(gid, name, ctx, &mut result, layers, psd)?;
        } else {
            // Try flat layer list first
            let layer = find_layer_by_parsed_name(layers, name);

            // Check if that layer also happens to be a group
            let group_id = layer.and_then(|l| {
                psd.groups()
                    .iter()
                    .find(|(_, g)| g.name() == l.name())
                    .map(|(&id, _)| id)
            });

            if let Some(gid) = group_id {
                self.process_group(gid, name, ctx, &mut result, layers, psd)?;
            } else if let Some(layer) = layer {
                self.process_single_layer(layer, name, ctx, &mut result, psd)?;
            }
        }

        Ok(result)
    }
}

impl BasicSprite {
    /// Merge all visible direct children of a group into a single sprite PNG.
    fn process_group(
        &self,
        gid: u32,
        name: &str,
        ctx: &SpriteContext,
        result: &mut Map<String, Value>,
        layers: &[psd::PsdLayer],
        psd: &psd::Psd,
    ) -> Result<()> {
        let lw = result.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let lh = result.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let lx = result.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let ly = result.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        // If bounds came through as zero (common for groups), compute from group_bounds
        let (lx, ly, lw, lh) = if lw == 0 || lh == 0 {
            if let Some((top, left, bottom, right)) = psd.group_bounds(gid) {
                (left, top, (right - left).max(0) as u32, (bottom - top).max(0) as u32)
            } else {
                (lx, ly, lw, lh)
            }
        } else {
            (lx, ly, lw, lh)
        };

        result.insert("width".into(), Value::from(lw));
        result.insert("height".into(), Value::from(lh));
        result.insert("x".into(), Value::from(lx));
        result.insert("y".into(), Value::from(ly));

        // Try to export mask from a matching pixel layer (if one exists)
        if let Some(layer) = find_layer_by_parsed_name(layers, name) {
            export_mask(
                layer,
                result,
                ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir),
                "sprites",
                name,
                ctx.config.metadata_only,
            )?;
        }

        if ctx.config.metadata_only || lw == 0 || lh == 0 {
            return Ok(());
        }

        let mut merged = RgbaImage::new(lw, lh);

        if let Some(sub_layers) = psd.get_group_sub_layers(&gid) {
            for child in sub_layers.iter() {
                if !child.visible() {
                    continue;
                }
                // Only direct children
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
        result.insert(
            "filePath".into(),
            Value::String(format!("sprites/{}", filename)),
        );

        Ok(())
    }

    /// Export a single pixel layer as a sprite PNG.
    fn process_single_layer(
        &self,
        layer: &psd::PsdLayer,
        name: &str,
        ctx: &SpriteContext,
        result: &mut Map<String, Value>,
        psd: &psd::Psd,
    ) -> Result<()> {
        let lw = layer.width() as u32;
        let lh = layer.height() as u32;
        result.insert("width".into(), Value::from(lw));
        result.insert("height".into(), Value::from(lh));

        export_mask(
            layer,
            result,
            ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir),
            "sprites",
            name,
            ctx.config.metadata_only,
        )?;

        if !ctx.config.metadata_only {
            if let Some(img) = layer_to_cropped_image(layer, psd.width(), psd.height()) {
                let filename = format!("{}.png", name);
                let filepath = ctx.sprite_output_dir.join(&filename);
                std::fs::create_dir_all(ctx.sprite_output_dir)?;
                image_export::save_png(&img, &filepath)?;
                result.insert(
                    "filePath".into(),
                    Value::String(format!("sprites/{}", filename)),
                );
            }
        }

        Ok(())
    }
}

fn find_layer_by_parsed_name<'a>(
    layers: &'a [psd::PsdLayer],
    parsed_name: &str,
) -> Option<&'a psd::PsdLayer> {
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
