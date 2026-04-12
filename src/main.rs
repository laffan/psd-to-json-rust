use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use psd_to_json::config;
use psd_to_json::processor;

#[derive(Parser, Debug)]
#[command(name = "psd-to-json")]
#[command(about = "Convert PSD files to JSON with exported image assets")]
struct Cli {
    /// Watch PSD files for changes and re-process automatically
    #[arg(long)]
    watch: bool,

    /// Only generate JSON metadata without processing images
    #[arg(long)]
    metadata_only: bool,

    /// Path to the directory containing psd-to-json.config
    #[arg(short, long, default_value = ".")]
    dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let base_dir = std::fs::canonicalize(&cli.dir)?;

    let mut config = config::load_config(&base_dir)?;

    if cli.watch {
        config.generate_on_save = true;
        println!("Watch mode enabled via command line argument");
    }

    if cli.metadata_only {
        config.metadata_only = true;
        println!("Metadata-only mode enabled - skipping image processing");
    }

    // Initial processing
    run_processing(&config, &base_dir)?;

    if config.generate_on_save {
        println!("Watching for PSD file changes...");
        watch_loop(&config, &base_dir)?;
    }

    Ok(())
}

fn run_processing(config: &config::Config, base_dir: &std::path::Path) -> Result<()> {
    let data = processor::process_all_psds(config, base_dir)?;
    processor::write_json_output(&data, config, base_dir)?;
    Ok(())
}

fn watch_loop(config: &config::Config, base_dir: &std::path::Path) -> Result<()> {
    use std::collections::HashMap;
    use std::time::Duration;

    let psd_paths: Vec<PathBuf> = config.resolve_psd_paths(base_dir);
    let mut last_modified: HashMap<PathBuf, u64> = HashMap::new();

    for path in &psd_paths {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(modified) = meta.modified() {
                let secs = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                last_modified.insert(path.clone(), secs);
            }
        }
    }

    loop {
        std::thread::sleep(Duration::from_secs(2));

        let mut changes_detected = false;
        for path in &psd_paths {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(modified) = meta.modified() {
                    let secs = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let prev = last_modified.get(path).copied().unwrap_or(0);
                    if secs > prev {
                        println!("Change detected in {}", path.display());
                        changes_detected = true;
                        last_modified.insert(path.clone(), secs);
                    }
                }
            }
        }

        if changes_detected {
            if let Err(e) = run_processing(config, base_dir) {
                eprintln!("Processing error: {}", e);
            }
            println!("Processing complete. Watching for more changes...");
        }
    }
}
