//! What the manifest says about a layer that is hidden in Photoshop.
//!
//! Hidden is a fact about how a layer is *drawn*, not about whether anyone
//! may have it, so by default the asset is exported exactly as before and the
//! entry carries `"visible": false`. A caller who means "leave it out" says so
//! with `hiddenLayers: "skip"`, and then a hidden group takes its contents
//! with it.
//!
//! The fixture is built rather than committed: the `psd` fork can write a
//! file, so a test can say precisely which layers are hidden instead of
//! depending on a binary nobody can read in a diff.

use psd::{GroupBuilder, LayerBuilder, PsdBuilder};
use psd_to_json::config::HiddenLayers;
use psd_to_json::{process_all_psds, Config};
use serde_json::Value;

/// A PSD holding a visible sprite, a hidden sprite, and a hidden group with a
/// visible sprite inside it.
fn fixture() -> Vec<u8> {
    let mut builder = PsdBuilder::new(64, 64);
    builder.add_layer(
        LayerBuilder::new("S | shown")
            .rgba(2, 2, vec![255; 16])
            .at(0, 0),
    );
    builder.add_layer(
        LayerBuilder::new("S | stashed")
            .rgba(2, 2, vec![255; 16])
            .at(8, 8)
            .visible(false),
    );
    builder.add_group(
        GroupBuilder::new("G | drafts").visible(false).add_layer(
            LayerBuilder::new("S | inside")
                .rgba(2, 2, vec![255; 16])
                .at(16, 16),
        ),
    );
    builder.to_bytes().expect("the fixture should write")
}

/// Run the pipeline over the fixture in a directory of its own.
fn run(hidden_layers: HiddenLayers) -> Value {
    let dir = std::env::temp_dir().join(format!(
        "psd-to-json-visibility-{:?}-{}",
        hidden_layers,
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp directory");
    std::fs::write(dir.join("fixture.psd"), fixture()).expect("the fixture on disk");

    let config = Config {
        output_dir: "assets".into(),
        psd_files: vec!["fixture.psd".into()],
        tile_slice_size: 512,
        tile_scaled_versions: Vec::new(),
        generate_on_save: false,
        png_quality_range: Default::default(),
        jpg_quality: 85,
        ignore_layers: Vec::new(),
        hidden_layers,
        // The entries are what this is about; the PNGs are not.
        metadata_only: true,
    };

    let all = process_all_psds(&config, &dir).expect("the fixture should process");
    let _ = std::fs::remove_dir_all(&dir);
    all.get("fixture").expect("the fixture's own entry").clone()
}

/// The layers of one manifest, by name, flattened.
fn layers(doc: &Value) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    walk(doc.get("layers"), &mut out);
    out
}

fn walk(node: Option<&Value>, out: &mut Vec<(String, Value)>) {
    let Some(Value::Array(items)) = node else { return };
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        out.push((name, item.clone()));
        walk(item.get("children"), out);
    }
}

fn named<'a>(all: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    all.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

#[test]
fn hidden_layers_are_exported_and_marked() {
    let all = layers(&run(HiddenLayers::Include));

    // Every layer is still there — hidden says how to draw it, not whether
    // the game may have it.
    assert!(named(&all, "shown").is_some());
    assert!(named(&all, "stashed").is_some());
    assert!(named(&all, "drafts").is_some());
    assert!(named(&all, "inside").is_some());

    // A visible layer says nothing at all, as it always has.
    assert_eq!(named(&all, "shown").unwrap().get("visible"), None);
    assert_eq!(
        named(&all, "stashed").unwrap().get("visible"),
        Some(&Value::Bool(false)),
    );
    assert_eq!(
        named(&all, "drafts").unwrap().get("visible"),
        Some(&Value::Bool(false)),
    );
    // A child of a hidden group says only whether *it* is hidden. Carrying
    // the group's answer down is the consumer's job.
    assert_eq!(named(&all, "inside").unwrap().get("visible"), None);
}

#[test]
fn skip_leaves_them_out_entirely() {
    let all = layers(&run(HiddenLayers::Skip));

    assert!(named(&all, "shown").is_some());
    assert!(named(&all, "stashed").is_none());
    // And a hidden group takes its contents with it.
    assert!(named(&all, "drafts").is_none());
    assert!(named(&all, "inside").is_none());
}
