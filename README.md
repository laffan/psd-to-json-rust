# PSD to JSON (Rust)

A Rust port of [psd-to-json](https://github.com/laffan/psd-to-json) — output the layers of a PSD as game assets by using a layer naming system. Generates optimized sprites, spritesheets, tiles, and a JSON manifest.

Built for deployment on iPads via [Tauri 2.0](https://v2.tauri.app/), with no system-level binary dependencies.

## Installation

```bash
cargo install --git https://github.com/laffan/psd-to-json-rust
```

Or add as a library dependency:

```toml
[dependencies]
psd-to-json = { git = "https://github.com/laffan/psd-to-json-rust" }
```

## Usage

```bash
# Process PSDs using config in current directory
psd-to-json

# Watch for changes and reprocess automatically
psd-to-json --watch

# Generate only JSON metadata (skip image export)
psd-to-json --metadata-only

# Specify a different working directory
psd-to-json --dir path/to/project
```

## Configuration File

Create a `psd-to-json.config` JSON file in your project directory:

```json
{
  "output_dir": "path/to/assets/folder",
  "psd_files": [
    "path/to/demo.psd"
  ],
  "tile_slice_size": 500,
  "tile_scaled_versions": [100],
  "generateOnSave": false,
  "pngQualityRange": {
    "low": 85,
    "high": 90
  },
  "jpgQuality": 80,
  "ignoreLayers": []
}
```

| Key | Type | Default | Description |
|---|---|---|---|
| `output_dir` | string | required | Output directory for all generated assets |
| `psd_files` | string[] | required | Array of PSD file paths to process |
| `tile_slice_size` | number | 512 | Pixel size for tile slicing |
| `tile_scaled_versions` | number[] | [] | Additional scaled tile sizes to generate |
| `generateOnSave` | bool | false | Enable file-watching mode |
| `pngQualityRange` | object | {low: 45, high: 65} | PNG quality range (for future optimization) |
| `jpgQuality` | number | 85 | JPEG quality for tile export |
| `ignoreLayers` | string[] | [] | Layer names to skip during processing |

## Layer Naming System

Layers are named with a pipe-delimited format:

```
category | name | type | attributes
```

| Pipes | Parsing |
|---|---|
| 0 | Layer ignored |
| 1 | Category and name |
| 2 | Category, name, and attributes |
| 3 | Category, name, type, and attributes |

### Categories

| Code | Category | Description |
|---|---|---|
| **G** | Group | Nested groups with optional attributes |
| **P** | Point | XY coordinate at layer center |
| **Z** | Zone | Bounding area with optional vector path data |
| **S** | Sprite | PNG image export (individual or flattened group) |
| **T** | Tileset | Diced images at configurable tile sizes |

### Sprite Types

| Type | Description |
|---|---|
| *(default)* | Single layer or merged group as one PNG |
| `animation` | Numbered child layers assembled into a spritesheet |
| `spritesheet` | Group children arranged in equal-sized grid cells |
| `atlas` | Children packed into a texture atlas with placement data |

**Examples:**
- `S | player` — basic sprite
- `S | walkCycle | animation |` — animation spritesheet
- `S | icons | spritesheet |` — grid spritesheet
- `S | uiElements | atlas |` — texture atlas

### Tile Types

| Type | Description |
|---|---|
| *(default)* | PNG tiles with transparency |
| `jpg` | JPEG tiles (smaller file size, no transparency) |

### Attributes

Comma-separated key:value pairs appended to the layer name. Values can be strings, integers, booleans, or arrays. Bare attribute names become `true` booleans.

```
P | enemy_spawn | level:5, isPrivate, style:"fancy", targets:["pandas","dogs","Bob"]
```

Produces:
```json
{
  "name": "enemy_spawn",
  "category": "point",
  "x": 100,
  "y": 200,
  "attributes": {
    "level": 5,
    "isPrivate": true,
    "style": "fancy",
    "targets": ["pandas", "dogs", "Bob"]
  }
}
```

## Layer Masks

If a layer or group has a layer mask in Photoshop, `psd-to-json` will automatically export the mask as a separate PNG file and include mask metadata in the JSON output:

```json
{
  "name": "myLayer",
  "mask": true,
  "maskX": 0,
  "maskY": 0,
  "maskWidth": 200,
  "maskHeight": 150,
  "maskPath": "sprites/myLayer_mask.png"
}
```

In `--metadata-only` mode, mask properties are recorded without generating image files.

## Output Structure

```
{output_dir}/
  {psd_name}/
    data.json
    sprites/
      {spriteName}.png
      {spriteName}_mask.png
    tiles/
      {tileName}/
        {tile_slice_size}/
          {tileName}_tile_0_0.png
          ...
        {scaled_size}/
          ...
    masks/
      {groupName}_mask.png
```

## Differences from the Python Version

This Rust port is designed for feature parity with the [Python original](https://github.com/laffan/psd-to-json). Key differences:

| Feature | Python | Rust |
|---|---|---|
| PSD parsing | [psd-tools](https://github.com/psd-tools/psd-tools) | [psd](https://github.com/laffan/psd) (fork) |
| Image processing | Pillow | [image](https://crates.io/crates/image) crate |
| PNG optimization | `pngquant` (external CLI) | Not yet integrated (planned: `imagequant` crate) |
| Tile parallelism | Sequential | Parallel via [rayon](https://crates.io/crates/rayon) |
| iPad/iOS support | No | Yes (no system dependencies) |
| Blend mode compositing | Not rendered (metadata only) | Not rendered (metadata only) |

### Known Limitations

- **Blend modes**: Non-normal blend modes are captured as metadata but do not affect merged sprite images. Most game assets use Normal blending.
- **PNG optimization**: Not yet integrated. The `imagequant` crate (pure Rust) is the planned replacement for `pngquant`.
- **Group mask pixel export**: Group mask metadata is recorded, but pixel export requires an upstream crate update.
- **Non-8-bit / non-RGB PSDs**: Only 8-bit RGB is supported. Game assets are typically 8-bit RGB.

See [GAPS.md](./GAPS.md) for the full feature gap analysis.

## Tauri 2.0 Integration

This crate is designed as both a CLI tool and a library. For Tauri integration, use the library API:

```rust
use psd_to_json::{Config, process_all_psds, write_json_output};

fn process(config: Config, base_dir: &std::path::Path) -> anyhow::Result<()> {
    let data = process_all_psds(&config, base_dir)?;
    write_json_output(&data, &config, base_dir)?;
    Ok(())
}
```

All image processing is pure Rust with no system dependencies, making it safe for iOS sandbox deployment.

## Credits

- PSD parsing: [psd crate](https://github.com/laffan/psd) (fork of [chinedufn/psd](https://github.com/chinedufn/psd))
- Original Python tool: [psd-to-json](https://github.com/laffan/psd-to-json)
- Texture packing algorithm: [TextureAtlas](https://github.com/Ezphares/TextureAtlas) (Ezphares)

## License

MIT
