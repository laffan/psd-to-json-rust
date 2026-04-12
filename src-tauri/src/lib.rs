use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, State};

/// Shared application state.
struct AppState {
    psd_path: Mutex<Option<PathBuf>>,
    output_dir: Mutex<Option<PathBuf>>,
}

// ── Tauri Commands ──────────────────────────────────────────────────

/// Set the selected PSD file path (called from frontend after dialog).
#[tauri::command]
fn set_psd_path(state: State<AppState>, path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("File not found: {}", path));
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    *state.psd_path.lock().unwrap() = Some(p);
    Ok(name)
}

/// Set the output directory path.
#[tauri::command]
fn set_output_dir(state: State<AppState>, path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    *state.output_dir.lock().unwrap() = Some(p.clone());
    Ok(p.display().to_string())
}

/// Get current selections.
#[tauri::command]
fn get_selections(state: State<AppState>) -> Selections {
    Selections {
        psd_path: state
            .psd_path
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.display().to_string()),
        output_dir: state
            .output_dir
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.display().to_string()),
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Selections {
    psd_path: Option<String>,
    output_dir: Option<String>,
}

/// Options sent from the frontend sidebar.
#[derive(Deserialize, Clone)]
struct ProcessOptions {
    tile_slice_size: Option<u32>,
    tile_scaled_versions: Option<Vec<u32>>,
    png_quality_low: Option<u8>,
    png_quality_high: Option<u8>,
    jpg_quality: Option<u8>,
    ignore_layers: Option<Vec<String>>,
    metadata_only: Option<bool>,
}

/// Run the PSD processing. Emits `log-line` events to the frontend for
/// terminal output, and returns the JSON result on success.
#[tauri::command]
async fn process_psd(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    options: ProcessOptions,
) -> Result<String, String> {
    let psd_path = state
        .psd_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("No PSD file selected")?;
    let output_dir = state
        .output_dir
        .lock()
        .unwrap()
        .clone()
        .ok_or("No output directory selected")?;

    let psd_path_str = psd_path.display().to_string();
    let output_dir_str = output_dir.display().to_string();

    emit_log(&app, &format!("Starting processing: {}", psd_path_str));

    // Build config from frontend options
    let config = psd_to_json::Config {
        output_dir: output_dir_str.clone(),
        psd_files: vec![psd_path_str.clone()],
        tile_slice_size: options.tile_slice_size.unwrap_or(512),
        tile_scaled_versions: options.tile_scaled_versions.clone().unwrap_or_default(),
        generate_on_save: false,
        png_quality_range: psd_to_json::config::PngQualityRange {
            low: options.png_quality_low.unwrap_or(45),
            high: options.png_quality_high.unwrap_or(65),
        },
        jpg_quality: options.jpg_quality.unwrap_or(85),
        ignore_layers: options.ignore_layers.clone().unwrap_or_default(),
        metadata_only: options.metadata_only.unwrap_or(false),
    };

    // Run processing in a blocking thread so we don't stall the async runtime.
    let app_handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let base_dir = Path::new("/");

        emit_log(&app_handle, "Parsing PSD file...");

        let psd_bytes = match std::fs::read(&psd_path) {
            Ok(b) => b,
            Err(e) => return Err(format!("Failed to read PSD: {}", e)),
        };

        emit_log(
            &app_handle,
            &format!(
                "PSD loaded ({:.1} MB)",
                psd_bytes.len() as f64 / 1_048_576.0
            ),
        );

        let psd = match psd::Psd::from_bytes(&psd_bytes) {
            Ok(p) => p,
            Err(e) => return Err(format!("Failed to parse PSD: {}", e)),
        };

        emit_log(
            &app_handle,
            &format!(
                "PSD parsed: {}x{}, {} layers, {} groups",
                psd.width(),
                psd.height(),
                psd.layers().len(),
                psd.groups().len()
            ),
        );

        // Create output directory
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| format!("Failed to create output dir: {}", e))?;

        let psd_name = psd_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let psd_output_dir = output_dir.join(&psd_name);
        std::fs::create_dir_all(&psd_output_dir)
            .map_err(|e| format!("Failed to create PSD output dir: {}", e))?;

        emit_log(&app_handle, "Processing layers...");

        let data = psd_to_json::process_all_psds(&config, base_dir)
            .map_err(|e| format!("Processing failed: {}", e))?;

        // Emit the layer tree diagram
        for (_psd_name, psd_data) in &data {
            if let Some(obj) = psd_data.as_object() {
                let tree = psd_to_json::format_layer_tree(obj);
                emit_log(&app_handle, "");
                emit_log(&app_handle, "LAYER_TREE_START");
                for line in tree.lines() {
                    emit_log(&app_handle, line);
                }
                emit_log(&app_handle, "LAYER_TREE_END");
                emit_log(&app_handle, "");
            }
        }

        emit_log(&app_handle, "Writing JSON output...");
        psd_to_json::write_json_output(&data, &config, base_dir)
            .map_err(|e| format!("JSON output failed: {}", e))?;

        let json_str = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Serialization failed: {}", e))?;

        emit_log(
            &app_handle,
            &format!("Done! Output written to {}", output_dir.display()),
        );

        Ok(json_str)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?;

    result
}

/// List all output files (images) in the output directory for the thumbnail view.
#[tauri::command]
fn list_output_files(state: State<AppState>) -> Result<Vec<OutputFile>, String> {
    let output_dir = state
        .output_dir
        .lock()
        .unwrap()
        .clone()
        .ok_or("No output directory selected")?;

    let psd_path = state.psd_path.lock().unwrap().clone();
    let psd_name = psd_path
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_default();

    let search_dir = if psd_name.is_empty() {
        output_dir
    } else {
        output_dir.join(&psd_name)
    };

    if !search_dir.exists() {
        return Ok(vec![]);
    }

    let mut files = Vec::new();
    collect_image_files(&search_dir, &search_dir, &mut files);
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

/// Read a thumbnail for a given file path, returning base64 encoded data.
#[tauri::command]
fn get_thumbnail(path: String, max_size: u32) -> Result<String, String> {
    let img = image::open(&path).map_err(|e| format!("Failed to open image: {}", e))?;

    let thumb = img.thumbnail(max_size, max_size);
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    thumb
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode thumbnail: {}", e))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(format!("data:image/png;base64,{}", b64))
}

// ── Helpers ─────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct OutputFile {
    absolute_path: String,
    relative_path: String,
    filename: String,
    is_json: bool,
}

fn collect_image_files(dir: &Path, base: &Path, out: &mut Vec<OutputFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_image_files(&path, base, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if matches!(ext_lower.as_str(), "png" | "jpg" | "jpeg" | "json") {
                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                out.push(OutputFile {
                    absolute_path: path.display().to_string(),
                    relative_path: relative,
                    filename,
                    is_json: ext_lower == "json",
                });
            }
        }
    }
}

/// Return the default output directory path.
/// On iOS this is the app's document directory (sandbox); on desktop it returns
/// an empty string so the frontend can show the folder picker instead.
#[tauri::command]
fn get_default_output_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    if cfg!(target_os = "ios") || cfg!(target_os = "android") {
        let doc_dir = app
            .path()
            .document_dir()
            .map_err(|e| format!("Failed to resolve document dir: {}", e))?;
        let output = doc_dir.join("psd-to-json-output");
        std::fs::create_dir_all(&output)
            .map_err(|e| format!("Failed to create output dir: {}", e))?;
        Ok(output.display().to_string())
    } else {
        Ok(String::new())
    }
}

fn emit_log(app: &tauri::AppHandle, message: &str) {
    let _ = app.emit("log-line", message.to_string());
}

// ── App builder (shared by desktop main and iOS entry point) ────────

fn build_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            psd_path: Mutex::new(None),
            output_dir: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            set_psd_path,
            set_output_dir,
            get_selections,
            get_default_output_dir,
            process_psd,
            list_output_files,
            get_thumbnail,
        ])
}

/// Desktop entry point — called from main.rs.
pub fn run() {
    build_app()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Mobile entry point — called by iOS/Android runtime.
#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn mobile_entry_point() {
    build_app()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
