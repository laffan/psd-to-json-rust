use serde_json::{Map, Value};

/// Process a point layer: adjust x/y to the center of the layer's bounding box,
/// and remove width/height from the output.
pub fn process_point(layer_info: &mut Map<String, Value>) {
    let x = layer_info.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = layer_info.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let w = layer_info.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let h = layer_info.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0);

    layer_info.insert("x".into(), Value::from(x + w / 2.0));
    layer_info.insert("y".into(), Value::from(y + h / 2.0));

    // Points don't include width/height in the Python version's output
    layer_info.remove("width");
    layer_info.remove("height");
}
