use anyhow::Result;
use image::RgbaImage;
use serde_json::{Map, Value};

use super::sprite::{layer_to_cropped_image, export_mask, SpriteContext, SpriteProcessor};
use crate::export::image_export;
use crate::parser::parse_layer_name;

pub struct AnimationSprite;

impl SpriteProcessor for AnimationSprite {
    fn process(
        &self,
        ctx: &SpriteContext,
        layers: &[psd::PsdLayer],
        psd: &psd::Psd,
    ) -> Result<Map<String, Value>> {
        let mut result = ctx.layer_info.clone();
        let name = ctx.layer_info.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");

        // Find the group — prefer the ID passed via context, fall back to name search
        let gid = match ctx.group_id.or_else(|| {
            psd.groups().iter().find(|(_, g)| {
                parse_layer_name(g.name())
                    .map(|p| p.name == name)
                    .unwrap_or(false)
            }).map(|(&id, _)| id)
        }) {
            Some(id) => id,
            None => {
                result.insert("note".into(), Value::String("Animation group not found".into()));
                return Ok(result);
            }
        };

        // Collect frames: children with integer names, sorted by name
        let mut frame_layers: Vec<&psd::PsdLayer> = Vec::new();
        if let Some(sub_layers) = psd.get_group_sub_layers(&gid) {
            for child in sub_layers.iter() {
                if child.parent_id() != Some(gid) {
                    continue;
                }
                // Check if name is an integer (animation frame numbering)
                if child.name().parse::<i32>().is_ok() {
                    frame_layers.push(child);
                }
            }
        }
        frame_layers.sort_by(|a, b| a.name().cmp(b.name()));

        if frame_layers.is_empty() {
            anyhow::bail!("No valid frames found in animation group '{}'", name);
        }

        // Calculate overall bounds across all frames
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_right = i32::MIN;
        let mut max_bottom = i32::MIN;

        for layer in &frame_layers {
            min_x = min_x.min(layer.layer_left());
            min_y = min_y.min(layer.layer_top());
            max_right = max_right.max(layer.layer_right());
            max_bottom = max_bottom.max(layer.layer_bottom());
        }

        let anim_width = (max_right - min_x) as u32;
        let anim_height = (max_bottom - min_y) as u32;
        let frame_count = frame_layers.len();
        let columns = (frame_count as f64).sqrt().ceil() as u32;
        let rows = ((frame_count as f64) / columns as f64).ceil() as u32;

        result.insert("frame_width".into(), Value::from(anim_width));
        result.insert("frame_height".into(), Value::from(anim_height));
        result.insert("frame_count".into(), Value::from(frame_count));
        result.insert("columns".into(), Value::from(columns));
        result.insert("rows".into(), Value::from(rows));
        result.insert("x".into(), Value::from(min_x));
        result.insert("y".into(), Value::from(min_y));
        result.insert("width".into(), Value::from(anim_width));
        result.insert("height".into(), Value::from(anim_height));

        // Export mask
        if let Some(layer) = layers.iter().find(|l| {
            parse_layer_name(l.name()).map(|p| p.name == name).unwrap_or(false)
        }) {
            export_mask(layer, &mut result, ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir), "sprites", name, ctx.config.metadata_only)?;
        }

        if !ctx.config.metadata_only && anim_width > 0 && anim_height > 0 {
            let sheet_w = columns * anim_width;
            let sheet_h = rows * anim_height;
            let mut sheet = RgbaImage::new(sheet_w, sheet_h);

            for (i, frame_layer) in frame_layers.iter().enumerate() {
                let col = (i as u32) % columns;
                let row = (i as u32) / columns;
                let cell_x = col * anim_width;
                let cell_y = row * anim_height;

                if let Some(frame_img) = layer_to_cropped_image(frame_layer, psd.width(), psd.height()) {
                    let offset_x = frame_layer.layer_left() - min_x;
                    let offset_y = frame_layer.layer_top() - min_y;
                    paste_image(&mut sheet, &frame_img, (cell_x as i32) + offset_x, (cell_y as i32) + offset_y);
                }
            }

            let filename = format!("{}.png", name);
            let filepath = ctx.sprite_output_dir.join(&filename);
            std::fs::create_dir_all(ctx.sprite_output_dir)?;
            image_export::save_png(&sheet, &filepath)?;
            result.insert("filePath".into(), Value::String(format!("sprites/{}", filename)));
        }

        Ok(result)
    }
}

fn paste_image(dst: &mut RgbaImage, src: &RgbaImage, dx: i32, dy: i32) {
    let (sw, sh) = src.dimensions();
    for sy in 0..sh {
        for sx in 0..sw {
            let tx = dx + sx as i32;
            let ty = dy + sy as i32;
            if tx >= 0 && ty >= 0 && (tx as u32) < dst.width() && (ty as u32) < dst.height() {
                let px = src.get_pixel(sx, sy);
                if px[3] > 0 {
                    dst.put_pixel(tx as u32, ty as u32, *px);
                }
            }
        }
    }
}
