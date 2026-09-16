//! What a group that has been composited says about its children.
//!
//! An atlas, a spritesheet, an animation and a plain merged sprite group are
//! each exported as *one* image built from what is inside them. psd-to-json's
//! README says it of every type that has children — "Child layer files are not
//! saved" — so the children are frames of that image rather than layers with
//! artwork of their own, and the manifest has no business listing them as
//! sprites or writing a PNG per child.
//!
//! The case that broke it is a file whose children carry the pipe convention.
//! Named by hand a frame is `purple`, which the parser ignores, so the extra
//! walk found nothing and the bug stayed hidden. An editor that prefixes every
//! layer it writes gives `S | purple`, which parses — and then one atlas
//! exported three PNGs, hung a `children` array off a layer consumers read as
//! a single image, and named its frames `"S | purple"`, which is a frame no
//! game code can ask for.
//!
//! Both spellings are tested, because both have to mean the same atlas.

use psd::{GroupBuilder, LayerBuilder, PsdBuilder};
use psd_to_json::config::HiddenLayers;
use psd_to_json::{process_all_psds, Config};
use serde_json::Value;

fn swatch(name: &str, x: i32, y: i32) -> LayerBuilder {
    LayerBuilder::new(name).rgba(2, 2, vec![255; 16]).at(x, y)
}

/// One file per naming style, holding the same four composited groups.
///
/// `prefix` is what goes in front of every child name: nothing for a file
/// named by hand, `S | ` for one an editor wrote.
fn fixture(prefix: &str) -> Vec<u8> {
    let child = |name: &str| format!("{prefix}{name}");
    let mut builder = PsdBuilder::new(64, 64);
    builder.add_group(
        GroupBuilder::new("S | confetti | atlas |")
            .add_layer(swatch(&child("purple"), 0, 0))
            .add_layer(swatch(&child("green"), 8, 8)),
    );
    builder.add_group(
        GroupBuilder::new("S | icons | spritesheet |")
            .add_layer(swatch(&child("star"), 16, 0))
            .add_layer(swatch(&child("moon"), 24, 0)),
    );
    builder.add_group(
        GroupBuilder::new("S | bounce | animation |")
            .add_layer(swatch(&child("2"), 32, 0))
            .add_layer(swatch(&child("10"), 40, 0))
            .add_layer(swatch(&child("1"), 48, 0)),
    );
    builder.add_group(
        GroupBuilder::new("S | merged")
            .add_layer(swatch(&child("top"), 0, 16))
            .add_layer(swatch(&child("bottom"), 8, 16)),
    );
    // A real group, for the contrast: this one keeps its children.
    builder.add_group(
        GroupBuilder::new("G | town").add_layer(swatch("S | roof", 0, 32)),
    );
    builder.to_bytes().expect("the fixture should write")
}

struct Run {
    doc: Value,
    /// Every PNG the run actually wrote, by file name.
    sprites: Vec<String>,
}

fn run(prefix: &str, tag: &str) -> Run {
    let dir = std::env::temp_dir().join(format!(
        "psd-to-json-composited-{}-{}",
        tag,
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp directory");
    std::fs::write(dir.join("fixture.psd"), fixture(prefix)).expect("the fixture on disk");

    let config = Config {
        output_dir: "assets".into(),
        psd_files: vec!["fixture.psd".into()],
        tile_slice_size: 512,
        tile_scaled_versions: Vec::new(),
        generate_on_save: false,
        png_quality_range: Default::default(),
        jpg_quality: 85,
        ignore_layers: Vec::new(),
        hidden_layers: HiddenLayers::Include,
        // The PNGs on disk are half of what this is about, so they are written.
        metadata_only: false,
    };

    let all = process_all_psds(&config, &dir).expect("the fixture should process");
    let mut sprites: Vec<String> = std::fs::read_dir(dir.join("assets/fixture/sprites"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    sprites.sort();
    let _ = std::fs::remove_dir_all(&dir);

    Run {
        doc: all.get("fixture").expect("the fixture's own entry").clone(),
        sprites,
    }
}

fn top<'a>(doc: &'a Value, name: &str) -> &'a Value {
    doc.get("layers")
        .and_then(Value::as_array)
        .expect("layers")
        .iter()
        .find(|l| l.get("name").and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no top-level layer called {name}"))
}

fn frame_keys(layer: &Value) -> Vec<&str> {
    layer
        .get("frames")
        .and_then(Value::as_object)
        .expect("a frames map")
        .keys()
        .map(String::as_str)
        .collect()
}

/// The whole contract, asserted identically for both naming styles.
fn assert_composited(run: &Run) {
    // No composited group keeps its children as layers.
    for name in ["confetti", "icons", "bounce", "merged"] {
        assert_eq!(
            top(&run.doc, name).get("children"),
            None,
            "{name} composites its children, so it should not list them as layers",
        );
    }

    // A real group still does.
    assert!(
        top(&run.doc, "town").get("children").is_some(),
        "G | town is a group and keeps its children",
    );

    // Frames are addressed by the name the author typed, either spelling.
    let mut atlas = frame_keys(top(&run.doc, "confetti"));
    atlas.sort_unstable();
    assert_eq!(atlas, ["green", "purple"]);

    let mut sheet = frame_keys(top(&run.doc, "icons"));
    sheet.sort_unstable();
    assert_eq!(sheet, ["moon", "star"]);

    // An instance names its frame the same way.
    let instances = top(&run.doc, "confetti")
        .get("instances")
        .and_then(Value::as_array)
        .expect("instances");
    let mut named: Vec<&str> = instances
        .iter()
        .filter_map(|i| i.get("name").and_then(Value::as_str))
        .collect();
    named.sort_unstable();
    assert_eq!(named, ["green", "purple"]);

    // The animation found all three frames, by number rather than by spelling.
    assert_eq!(
        top(&run.doc, "bounce").get("frame_count"),
        Some(&Value::from(3)),
    );

    // One PNG per composited group, one for the group's own sprite, and
    // nothing at all for a child.
    assert_eq!(
        run.sprites,
        vec![
            "bounce.png".to_string(),
            "confetti.png".to_string(),
            "icons.png".to_string(),
            "merged.png".to_string(),
            "roof.png".to_string(),
        ],
        "a composited group writes its own image and none of its children's",
    );
}

#[test]
fn plainly_named_children_are_consumed() {
    assert_composited(&run("", "plain"));
}

#[test]
fn pipe_named_children_are_consumed_too() {
    assert_composited(&run("S | ", "piped"));
}

/// The two spellings have to produce the same manifest, not merely two
/// manifests that each pass the assertions above.
#[test]
fn both_spellings_agree() {
    assert_eq!(run("", "plain-a").doc, run("S | ", "piped-a").doc);
}
