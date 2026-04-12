use anyhow::Result;
use psd::{BlendMode, Psd};
use serde_json::{Map, Value};
use std::path::Path;

use crate::config::Config;
use crate::parser::parse_layer_name;
use crate::types::point::process_point;
use crate::types::sprite::{create_sprite_processor, SpriteContext};
use crate::types::tiles;
use crate::types::zone::process_zone;

/// Process all PSD files specified in the config.
/// Returns a map of psd_name -> processed JSON data.
pub fn process_all_psds(config: &Config, base_dir: &Path) -> Result<Map<String, Value>> {
    let mut all_data = Map::new();

    for psd_path_str in &config.psd_files {
        let psd_path = base_dir.join(psd_path_str);
        if !psd_path.exists() {
            eprintln!("Warning: PSD file not found: {}", psd_path.display());
            continue;
        }

        let psd_name = psd_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let psd_output_dir = base_dir.join(&config.output_dir).join(&psd_name);
        std::fs::create_dir_all(&psd_output_dir)?;

        println!("Processing {}...", psd_path.display());
        let psd_bytes = std::fs::read(&psd_path)?;
        let psd = Psd::from_bytes(&psd_bytes).map_err(|e| anyhow::anyhow!("Failed to parse PSD: {}", e))?;

        let psd_data = process_psd(&psd, config, &psd_name, &psd_output_dir)?;
        all_data.insert(psd_name, Value::Object(psd_data));
    }

    Ok(all_data)
}

/// Process a single PSD file, returning structured JSON data.
fn process_psd(
    psd: &Psd,
    config: &Config,
    psd_name: &str,
    psd_output_dir: &Path,
) -> Result<Map<String, Value>> {
    let mut depth_counter: u32 = 0;
    let layers = process_layers(psd, config, psd_name, psd_output_dir, None, &mut depth_counter)?;

    // Reverse depth values
    let max_depth = if depth_counter > 0 { depth_counter - 1 } else { 0 };
    let mut layers_val = Value::Array(layers);
    reverse_depth(&mut layers_val, max_depth);

    let mut psd_data = Map::new();
    psd_data.insert("name".into(), Value::String(psd_name.to_string()));
    psd_data.insert("width".into(), Value::from(psd.width()));
    psd_data.insert("height".into(), Value::from(psd.height()));
    psd_data.insert("tile_slice_size".into(), Value::from(config.tile_slice_size));
    psd_data.insert(
        "tile_scaled_versions".into(),
        Value::Array(config.tile_scaled_versions.iter().map(|&v| Value::from(v)).collect()),
    );
    psd_data.insert("layers".into(), layers_val);

    Ok(psd_data)
}

/// Recursively process layers, building the JSON layer tree.
fn process_layers(
    psd: &Psd,
    config: &Config,
    psd_name: &str,
    psd_output_dir: &Path,
    parent_group_id: Option<u32>,
    depth_counter: &mut u32,
) -> Result<Vec<Value>> {
    let mut result_layers = Vec::new();
    let all_layers = psd.layers();
    let groups = psd.groups();

    // Collect items at this level: layers with parent_group_id matching, and groups with matching parent
    // We need to process in reverse order (bottom-to-top in PSD, which is the natural iteration order reversed)

    // Gather layers at this nesting level
    let mut items_at_level: Vec<LevelItem> = Vec::new();

    // Add pixel layers at this level
    for (idx, layer) in all_layers.iter().enumerate() {
        if layer.parent_id() == parent_group_id {
            items_at_level.push(LevelItem::Layer(idx));
        }
    }

    // Add groups at this level
    for &gid in psd.group_ids_in_order() {
        let group = &groups[&gid];
        if group.parent_id() == parent_group_id {
            items_at_level.push(LevelItem::Group(gid));
        }
    }

    // Process in reverse order to match the Python behavior (bottom-to-top visual stacking)
    items_at_level.reverse();

    for item in &items_at_level {
        match item {
            LevelItem::Layer(idx) => {
                let layer = &all_layers[*idx];
                let parsed = match parse_layer_name(layer.name()) {
                    Some(p) => p,
                    None => continue,
                };

                if config.ignore_layers.contains(&parsed.name) {
                    continue;
                }

                let mut layer_info = build_layer_info(&parsed, layer, *depth_counter);
                *depth_counter += 1;

                match parsed.category.as_str() {
                    "point" => {
                        process_point(&mut layer_info);
                    }
                    "zone" => {
                        process_zone(&mut layer_info, layer, psd.width(), psd.height());
                    }
                    "tileset" => {
                        let tile_info = tiles::process_tiles(layer, &layer_info, config, psd_output_dir, psd)?;
                        // Merge tile_info into layer_info
                        for (k, v) in tile_info {
                            layer_info.insert(k, v);
                        }
                    }
                    "sprite" => {
                        let sprite_type = parsed.layer_type.as_deref();
                        let processor = create_sprite_processor(sprite_type);
                        let sprite_output_dir = psd_output_dir.join("sprites");
                        let ctx = SpriteContext {
                            layer_info: &layer_info,
                            config,
                            sprite_output_dir: &sprite_output_dir,
                        };
                        match processor.process(&ctx, all_layers, psd) {
                            Ok(sprite_info) => {
                                for (k, v) in sprite_info {
                                    layer_info.insert(k, v);
                                }
                            }
                            Err(e) => {
                                layer_info.insert("note".into(), Value::String(format!("Sprite processing error: {}", e)));
                            }
                        }
                    }
                    _ => {}
                }

                // Capture alpha/opacity
                capture_layer_properties(layer, &mut layer_info);

                result_layers.push(Value::Object(layer_info));
            }
            LevelItem::Group(gid) => {
                let group = &groups[gid];
                let parsed = match parse_layer_name(group.name()) {
                    Some(p) => p,
                    None => continue,
                };

                if config.ignore_layers.contains(&parsed.name) {
                    continue;
                }

                // For groups, get bounds from group_bounds
                let (x, y, w, h) = if let Some((top, left, bottom, right)) = psd.group_bounds(*gid) {
                    (left, top, (right - left).max(0), (bottom - top).max(0))
                } else {
                    (group.layer_left(), group.layer_top(), group.width() as i32, group.height() as i32)
                };

                let mut layer_info = Map::new();
                layer_info.insert("name".into(), Value::String(parsed.name.clone()));
                layer_info.insert("category".into(), Value::String(parsed.category.clone()));
                layer_info.insert("x".into(), Value::from(x));
                layer_info.insert("y".into(), Value::from(y));
                layer_info.insert("width".into(), Value::from(w));
                layer_info.insert("height".into(), Value::from(h));
                layer_info.insert("initialDepth".into(), Value::from(*depth_counter));
                *depth_counter += 1;

                if let Some(ref lt) = parsed.layer_type {
                    layer_info.insert("type".into(), Value::String(lt.clone()));
                }

                // Add attributes
                if !parsed.attributes.is_empty() {
                    let attrs: Map<String, Value> = parsed.attributes.into_iter().collect();
                    layer_info.insert("attributes".into(), Value::Object(attrs));
                }

                // Handle sprite groups (S | name | type)
                if parsed.category == "sprite" {
                    let sprite_type = parsed.layer_type.as_deref();
                    let processor = create_sprite_processor(sprite_type);
                    let sprite_output_dir = psd_output_dir.join("sprites");
                    let ctx = SpriteContext {
                        layer_info: &layer_info,
                        config,
                        sprite_output_dir: &sprite_output_dir,
                    };
                    match processor.process(&ctx, all_layers, psd) {
                        Ok(sprite_info) => {
                            for (k, v) in sprite_info {
                                layer_info.insert(k, v);
                            }
                        }
                        Err(e) => {
                            layer_info.insert("note".into(), Value::String(format!("Sprite processing error: {}", e)));
                        }
                    }
                }

                // Recursively process children
                let children = process_layers(psd, config, psd_name, psd_output_dir, Some(*gid), depth_counter)?;
                if !children.is_empty() {
                    layer_info.insert("children".into(), Value::Array(children));
                }

                // Export mask for group layers
                if parsed.category == "group" {
                    export_group_mask(group, &mut layer_info, psd_output_dir, &parsed.name, config)?;
                }

                // Capture opacity and blend mode from group
                capture_group_properties(group, &mut layer_info);

                result_layers.push(Value::Object(layer_info));
            }
        }
    }

    Ok(result_layers)
}

enum LevelItem {
    Layer(usize),
    Group(u32),
}

fn build_layer_info(parsed: &crate::parser::ParsedLayer, layer: &psd::PsdLayer, depth: u32) -> Map<String, Value> {
    let mut info = Map::new();
    info.insert("name".into(), Value::String(parsed.name.clone()));
    info.insert("category".into(), Value::String(parsed.category.clone()));
    info.insert("x".into(), Value::from(layer.layer_left()));
    info.insert("y".into(), Value::from(layer.layer_top()));
    info.insert("width".into(), Value::from(layer.width()));
    info.insert("height".into(), Value::from(layer.height()));
    info.insert("initialDepth".into(), Value::from(depth));

    if let Some(ref lt) = parsed.layer_type {
        info.insert("type".into(), Value::String(lt.clone()));
    }

    if !parsed.attributes.is_empty() {
        let attrs: Map<String, Value> = parsed.attributes.clone().into_iter().collect();
        info.insert("attributes".into(), Value::Object(attrs));
    }

    info
}

fn capture_layer_properties(layer: &psd::PsdLayer, info: &mut Map<String, Value>) {
    // Opacity
    if layer.opacity() != 255 {
        let alpha = (layer.opacity() as f64 / 255.0 * 100.0).round() / 100.0;
        info.insert("alpha".into(), Value::from(alpha));
    }

    // Blend mode
    let bm = layer.blend_mode();
    if !matches!(bm, BlendMode::PassThrough | BlendMode::Normal) {
        info.insert("blendMode".into(), Value::String(format!("{:?}", bm)));
    }
}

fn capture_group_properties(group: &psd::PsdGroup, info: &mut Map<String, Value>) {
    if group.opacity() != 255 {
        let alpha = (group.opacity() as f64 / 255.0 * 100.0).round() / 100.0;
        info.insert("alpha".into(), Value::from(alpha));
    }

    let bm = group.blend_mode();
    if !matches!(bm, BlendMode::PassThrough | BlendMode::Normal) {
        info.insert("blendMode".into(), Value::String(format!("{:?}", bm)));
    }
}

fn export_group_mask(
    group: &psd::PsdGroup,
    layer_info: &mut Map<String, Value>,
    _psd_output_dir: &Path,
    _name: &str,
    config: &Config,
) -> Result<()> {
    let mask = match group.mask() {
        Some(m) => m,
        None => return Ok(()),
    };

    layer_info.insert("mask".into(), Value::Bool(true));
    layer_info.insert("maskX".into(), Value::from(mask.left));
    layer_info.insert("maskY".into(), Value::from(mask.top));
    layer_info.insert("maskWidth".into(), Value::from(mask.width()));
    layer_info.insert("maskHeight".into(), Value::from(mask.height()));

    if config.metadata_only {
        return Ok(());
    }

    // Note: PsdGroup does not currently expose mask_pixels() in the psd crate.
    // The mask metadata is still recorded; pixel export would require crate extension.
    // For now, we just record the metadata.

    Ok(())
}

fn reverse_depth(value: &mut Value, max_depth: u32) {
    match value {
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                reverse_depth(item, max_depth);
            }
        }
        Value::Object(obj) => {
            if let Some(depth_val) = obj.get_mut("initialDepth") {
                if let Some(d) = depth_val.as_u64() {
                    *depth_val = Value::from(max_depth as u64 - d);
                }
            }
            if let Some(children) = obj.get_mut("children") {
                reverse_depth(children, max_depth);
            }
        }
        _ => {}
    }
}

/// Write the processed data to JSON files.
pub fn write_json_output(data: &Map<String, Value>, config: &Config, base_dir: &Path) -> Result<()> {
    let output_dir = base_dir.join(&config.output_dir);

    for (psd_name, psd_data) in data {
        let psd_output_dir = output_dir.join(psd_name);
        std::fs::create_dir_all(&psd_output_dir)?;

        let output_file = psd_output_dir.join("data.json");
        let json_string = serde_json::to_string_pretty(psd_data)?;
        std::fs::write(&output_file, json_string)?;

        println!("JSON file generated: {}", output_file.display());
    }

    println!("All JSON files generated in {}", output_dir.display());
    Ok(())
}
