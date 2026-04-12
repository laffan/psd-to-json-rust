use anyhow::Result;
use image::RgbaImage;
use serde_json::{Map, Value};

use super::sprite::{layer_to_cropped_image, export_mask, SpriteContext, SpriteProcessor};
use crate::export::image_export;
use crate::parser::parse_layer_name;

pub struct AtlasSprite;

impl SpriteProcessor for AtlasSprite {
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
                result.insert("note".into(), Value::String("Atlas group not found".into()));
                return Ok(result);
            }
        };

        // Collect unique frames from visible children
        let mut frames: Vec<AtlasFrame> = Vec::new();
        let mut instances: Vec<Value> = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        if let Some(sub_layers) = psd.get_group_sub_layers(&gid) {
            for child in sub_layers.iter() {
                if !child.visible() {
                    continue;
                }
                if child.parent_id() != Some(gid) {
                    continue;
                }

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
                    let img = if ctx.config.metadata_only {
                        None
                    } else {
                        layer_to_cropped_image(child, psd.width(), psd.height())
                    };
                    frames.push(AtlasFrame {
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

        result.insert("type".into(), Value::String("atlas".into()));

        // Export mask
        if let Some(layer) = layers.iter().find(|l| {
            parse_layer_name(l.name()).map(|p| p.name == name).unwrap_or(false)
        }) {
            export_mask(layer, &mut result, ctx.sprite_output_dir.parent().unwrap_or(ctx.sprite_output_dir), "sprites", name, ctx.config.metadata_only)?;
        }

        if ctx.config.metadata_only {
            result.insert("instances".into(), Value::Array(instances));
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
            // Simple horizontal strip layout (matches Python's AtlasSprite._create_atlas)
            let total_width: u32 = frames.iter().map(|f| f.width).sum();
            let max_height: u32 = frames.iter().map(|f| f.height).max().unwrap_or(1);

            let mut atlas = RgbaImage::new(total_width, max_height);
            let mut x_offset: u32 = 0;
            let mut frames_map = Map::new();

            for frame in &frames {
                if let Some(ref img) = frame.image {
                    paste_image(&mut atlas, img, x_offset as i32, 0);
                }

                frames_map.insert(frame.name.clone(), serde_json::json!({
                    "x": x_offset,
                    "y": 0,
                    "width": frame.width,
                    "height": frame.height,
                }));

                x_offset += frame.width;
            }

            result.insert("instances".into(), Value::Array(instances));
            result.insert("frames".into(), Value::Object(frames_map));

            let filename = format!("{}.png", name);
            let filepath = ctx.sprite_output_dir.join(&filename);
            std::fs::create_dir_all(ctx.sprite_output_dir)?;
            image_export::save_png(&atlas, &filepath)?;
            result.insert("filePath".into(), Value::String(format!("sprites/{}", filename)));
        }

        Ok(result)
    }
}

struct AtlasFrame {
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
