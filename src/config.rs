use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub output_dir: String,
    pub psd_files: Vec<String>,
    #[serde(default = "default_tile_slice_size")]
    pub tile_slice_size: u32,
    #[serde(default)]
    pub tile_scaled_versions: Vec<u32>,
    #[serde(default, rename = "generateOnSave")]
    pub generate_on_save: bool,
    #[serde(default = "default_png_quality_range", rename = "pngQualityRange")]
    pub png_quality_range: PngQualityRange,
    #[serde(default = "default_jpg_quality", rename = "jpgQuality")]
    pub jpg_quality: u8,
    #[serde(default, rename = "ignoreLayers")]
    pub ignore_layers: Vec<String>,
    /// What to do with a layer that is hidden in Photoshop.
    #[serde(default, rename = "hiddenLayers")]
    pub hidden_layers: HiddenLayers,
    #[serde(skip)]
    pub metadata_only: bool,
}

/// What becomes of a layer whose eye is off in Photoshop.
///
/// Hidden is not the same as absent, and which of the two it means is the
/// caller's to say. A game editor wants the asset exported and the manifest
/// to say the layer is hidden, so it can be placed and turned on later; a
/// project that uses hidden layers as scratch wants them gone from the output
/// entirely. Neither is a safe guess, so it is a setting.
///
/// `Include` is the default because it is what this tool has always done —
/// visibility was simply never read — and because it loses nothing: a
/// consumer that does not know about `visible` behaves exactly as before.
///
/// Neither value says anything about a *merged* group. An `S | name` group, a
/// tileset and an atlas are composited into one image, and compositing has
/// always skipped hidden children the way Photoshop does — there is no
/// separate asset there to export, or to turn on later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HiddenLayers {
    /// Export them as usual, and mark them `"visible": false`.
    #[default]
    Include,
    /// Leave them out: no asset, no entry, and no children of a hidden group.
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PngQualityRange {
    pub low: u8,
    pub high: u8,
}

fn default_tile_slice_size() -> u32 {
    512
}

fn default_png_quality_range() -> PngQualityRange {
    PngQualityRange { low: 45, high: 65 }
}

fn default_jpg_quality() -> u8 {
    85
}

impl Default for PngQualityRange {
    fn default() -> Self {
        default_png_quality_range()
    }
}

pub fn load_config(base_dir: &Path) -> anyhow::Result<Config> {
    let config_path = base_dir.join("psd-to-json.config");
    if !config_path.exists() {
        anyhow::bail!(
            "No psd-to-json.config file found in {}.\n\n\
             Please create a psd-to-json.config file with the following structure:\n\
             {{\n\
             \x20 \"output_dir\": \"path/to/assets/folder\",\n\
             \x20 \"psd_files\": [\"path/to/demo.psd\"],\n\
             \x20 \"tile_slice_size\": 500,\n\
             \x20 \"tile_scaled_versions\": [100],\n\
             \x20 \"generateOnSave\": false,\n\
             \x20 \"pngQualityRange\": {{ \"low\": 85, \"high\": 90 }},\n\
             \x20 \"jpgQuality\": 80,\n\
             \x20 \"ignoreLayers\": [],\n\
             \x20 \"hiddenLayers\": \"include\"\n\
             }}",
            base_dir.display()
        );
    }

    let contents = std::fs::read_to_string(&config_path)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

impl Config {
    /// Resolve PSD file paths relative to a base directory.
    pub fn resolve_psd_paths(&self, base_dir: &Path) -> Vec<PathBuf> {
        self.psd_files
            .iter()
            .map(|p| base_dir.join(p))
            .collect()
    }
}
