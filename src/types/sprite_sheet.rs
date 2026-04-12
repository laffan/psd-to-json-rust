use anyhow::Result;
use image::RgbaImage;
use serde_json::{Map, Value};

use super::sprite::{layer_to_cropped_image, export_mask, SpriteContext, SpriteProcessor};
use crate::export::image_export;
use crate::parser::parse_layer_name;

pub struct SpritesheetSprite;

impl SpriteProcessor for SpritesheetSprite {
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
                result.insert("note".into(), Value::String("Spritesheet group not found".into()));
                return Ok(result);
            }
        };

        // Collect unique frames from visible children
        let mut frames: Vec<FrameInfo> = Vec::new();
        let mut instances: Vec<Value> = Vec::new();
        let mut max_width: u32 = 0;
        let mut max_height: u32 = 0;
        let mut seen_names = std::collections::HashSet::new();
        if let Some(sub_layers) = psd.get_group_sub_layers(&gid) {
            // Reverse iteration to match Python's bottom-to-top child ordering
            // for spritesheet frame collection.
            let children: Vec<_> = sub_layers.iter()
                .filter(|c| c.visible() && c.parent_id() == Some(gid))
                .collect();
            for child in children.iter().rev() {

                let child_name = child.name().to_string();
                let cw = child.width() as u32;
                let ch = child.height() as u32;

                instances.push(serde_json::json!({
                    "name": child_name,
                    "x": child.layer_left(),
                    "y": child.layer_top(),
                }));

                if !seen_names.contains(&child_name) {
                    seen_names.insert(child_name.clone());
                    max_width = max_width.max(cw);
                    max_height = max_height.max(ch);
                    let img = if ctx.config.metadata_only {
                        None
                    } else {
                        layer_to_cropped_image(child, psd.width(), psd.height())
                    };
                    frames.push(FrameInfo {
                        name: child_name,
                        image: img,
                        width: cw,
                        height: ch,
                    });
                }
            }
        }

        if frames.is_empty() {
            result.insert("note".into(), Value::String("No valid frames found".into()));
            return Ok(result);
        }

        let frame_count = frames.len();
        let columns = (frame_count as f64).sqrt().ceil() as u32;
        let rows = ((frame_count as f64) / columns as f64).ceil() as u32;

        result.insert("frame_width".into(), Value::from(max_width));
        result.insert("frame_height".into(), Value::from(max_height));
        result.insert("frame_count".into(), Value::from(frame_count));
        result.insert("columns".into(), Value::from(columns));
        result.insert("rows".into(), Value::from(rows));
        result.insert("instances".into(), Value::Array(instances));

        // Export mask if available
        if let Some(layer) = layers.iter().find(|l| {
            parse_layer_name(l.name()).map(|p| p.name == name).unwrap_or(false)
        }) {
            export_mask(layer, &mut result, ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir), "sprites", name, ctx.config.metadata_only)?;
        }

        if ctx.config.metadata_only {
            let frames_map: Map<String, Value> = frames
                .iter()
                .map(|f| {
                    (f.name.clone(), serde_json::json!({
                        "width": f.width,
                        "height": f.height,
                    }))
                })
                .collect();
            result.insert("frames".into(), Value::Object(frames_map));
        } else {
            // Create spritesheet image
            let sheet_w = columns * max_width;
            let sheet_h = rows * max_height;
            let mut sheet = RgbaImage::new(sheet_w, sheet_h);

            let mut frames_map = Map::new();
            for (i, frame) in frames.iter().enumerate() {
                let col = (i as u32) % columns;
                let row = (i as u32) / columns;
                let sx = col * max_width;
                let sy = row * max_height;

                if let Some(ref img) = frame.image {
                    let offset_x = (max_width - frame.width) / 2;
                    let offset_y = (max_height - frame.height) / 2;
                    paste_image(&mut sheet, img, (sx + offset_x) as i32, (sy + offset_y) as i32);
                }

                frames_map.insert(frame.name.clone(), serde_json::json!({
                    "x": sx,
                    "y": sy,
                    "width": frame.width,
                    "height": frame.height,
                }));
            }
            result.insert("frames".into(), Value::Object(frames_map));

            let filename = format!("{}.png", name);
            let filepath = ctx.sprite_output_dir.join(&filename);
            std::fs::create_dir_all(ctx.sprite_output_dir)?;
            image_export::save_png(&sheet, &filepath)?;
            result.insert("filePath".into(), Value::String(format!("sprites/{}", filename)));
        }

        Ok(result)
    }
}

struct FrameInfo {
    name: String,
    image: Option<RgbaImage>,
    width: u32,
    height: u32,
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
