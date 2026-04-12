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
    #[serde(skip)]
    pub metadata_only: bool,
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
             \x20 \"ignoreLayers\": []\n\
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
