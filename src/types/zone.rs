use psd::PsdLayer;
use serde_json::{Map, Value};

/// Process a zone layer: extract vector mask subpaths (as pixel coordinates)
/// or fall back to width/height if no vector mask is present.
pub fn process_zone(layer_info: &mut Map<String, Value>, layer: &PsdLayer, psd_width: u32, psd_height: u32) {
    if layer.has_vector_mask() {
        if let Some(vm) = layer.vector_mask() {
            let mut subpaths = Vec::new();
            for subpath in &vm.paths {
                let points: Vec<Value> = subpath
                    .knots
                    .iter()
                    .map(|knot| {
                        let px = (knot.anchor.x * psd_width as f64) as i32;
                        let py = (knot.anchor.y * psd_height as f64) as i32;
                        Value::Array(vec![Value::from(px), Value::from(py)])
                    })
                    .collect();
                subpaths.push(Value::Array(points));
            }
            layer_info.insert("subpaths".into(), Value::Array(subpaths));

            let mut bbox = Map::new();
            bbox.insert("left".into(), Value::from(layer.layer_left()));
            bbox.insert("top".into(), Value::from(layer.layer_top()));
            bbox.insert("right".into(), Value::from(layer.layer_right()));
            bbox.insert("bottom".into(), Value::from(layer.layer_bottom()));
            layer_info.insert("bbox".into(), Value::Object(bbox));
        }
    }
    // If no vector mask, width and height are already in layer_info from the processor.
}
