# psd-to-json: Python-to-Rust Feature Gap Analysis

## Overview

This document compares the features of the Python
[psd-to-json](https://github.com/laffan/psd-to-json) tool against what the
Rust [psd crate fork](https://github.com/laffan/psd) currently supports, and
identifies gaps, workarounds, and Tauri/iPad deployment considerations.

---

## 1. Python psd-to-json Feature Inventory

### 1.1 Layer Naming Convention Parser

| Feature | Detail |
|---|---|
| Pipe-delimited names | `category \| name \| type \| attributes` |
| Category codes | G (group), P (point), Z (zone), S (sprite), T (tileset) |
| Attribute parsing | `key:value` pairs, booleans, arrays, nested objects |
| Layers without pipes | Silently ignored |
| `ignoreLayers` config | Named layers can be excluded |

### 1.2 Layer Categories & Processing

| Category | What it produces |
|---|---|
| **Point (P)** | XY coordinate at the layer's center; no image export |
| **Zone (Z)** | Bounding box or vector-mask subpaths (Bezier knots as pixel coords) |
| **Sprite (S)** | PNG image export + metadata. Sub-types: `basic`, `spritesheet`, `animation`, `atlas` |
| **Tileset (T)** | Sliced tile grid (PNG or JPG) at configurable tile size + scaled variants |
| **Group (G)** | Recursive container with children; optional mask export |

### 1.3 Sprite Sub-types

| Sub-type | Behavior |
|---|---|
| **basic** | Single layer or merged group -> one PNG |
| **spritesheet** | Group children arranged into a grid spritesheet PNG |
| **animation** | Numbered child layers -> spritesheet with frame bounds |
| **atlas** | Texture-packed children with per-frame + placement metadata |

### 1.4 Metadata Extracted per Layer

- `name`, `category`, `x`, `y`, `width`, `height`
- `initialDepth` (reversed z-order index)
- `type` (sprite sub-type)
- Custom `attributes` dict (from pipe-delimited name)
- `alpha` (opacity as 0.0-1.0 if != 100%)
- `blendMode` (name string if not Normal/PassThrough)
- `mask`, `maskX`, `maskY`, `maskWidth`, `maskHeight`, `maskPath`
- `children` (for groups)
- `filePath` (for exported images)
- Sprite-specific: `frame_width`, `frame_height`, `frame_count`, `columns`, `rows`, `frames`, `instances`
- Tileset-specific: `columns`, `rows`, `filetype`

### 1.5 Image Export

- Individual sprite PNGs
- Spritesheet PNGs (grid layout)
- Animation spritesheet PNGs
- Atlas PNGs (texture-packed)
- Tile PNGs or JPGs at base size + scaled versions
- Layer mask PNGs (for sprites, groups, and tilesets)
- PNG optimization via `pngquant`

### 1.6 Top-level JSON Output

```json
{
  "name": "psd_filename",
  "width": 4096,
  "height": 4096,
  "tile_slice_size": 500,
  "tile_scaled_versions": [100],
  "layers": [ ... ]
}
```

### 1.7 CLI / Configuration

- `psd-to-json.config` JSON file with: `output_dir`, `psd_files`, `tile_slice_size`,
  `tile_scaled_versions`, `generateOnSave`, `pngQualityRange`, `jpgQuality`, `ignoreLayers`
- `--watch` flag (poll every 2s for PSD changes)
- `--metadata-only` flag (skip all image processing)

---

## 2. Rust psd Crate (laffan/psd Fork) Capabilities

### 2.1 What Works

| Capability | API |
|---|---|
| Parse PSD from bytes | `Psd::from_bytes(&[u8])` |
| Document dimensions | `psd.width()`, `psd.height()` |
| Color mode, depth | `psd.color_mode()`, `psd.depth()` |
| Layer iteration | `psd.layers()`, `psd.layer_by_name()`, `psd.layer_by_idx()` |
| Layer properties | `name()`, `width()`, `height()`, `layer_top()`, `layer_left()`, `layer_bottom()`, `layer_right()` |
| Visibility, opacity | `visible()`, `opacity()` |
| Blend mode | `blend_mode()` -> `BlendMode` enum (28 modes) |
| Group hierarchy | `psd.groups()`, `psd.group_ids_in_order()`, `psd.get_group_sub_layers()` |
| Parent tracking | `layer.parent_id()` -> `Option<u32>` (group ID) |
| Clipping mask flag | `is_clipping_mask()` |
| Raw layer pixels | `layer.rgba()` -> `Vec<u8>` (no opacity/mask) |
| Composited pixels | `layer.composite_rgba()` -> `Vec<u8>` (with opacity + raster mask) |
| Raster mask metadata | `layer.mask()` -> `Option<&LayerMask>` with `top`, `left`, `bottom`, `right`, `width()`, `height()` |
| Raster mask pixels | `layer.mask_pixels()` -> grayscale buffer |
| Vector mask data | `layer.vector_mask()` -> `Option<&VectorMask>` with `Subpath`/`BezierKnot`/`PathPoint` |
| Group bounds | `psd.group_bounds(id)` -> `Option<(i32, i32, i32, i32)>` |
| Flatten with filter | `psd.flatten_layers_rgba(filter)` |
| Compression info | `layer.compression()`, `psd.compression()` |
| Channel compression | RLE and Raw supported |

### 2.2 Known Limitations (from UPDATES.md)

| Limitation | Impact on psd-to-json |
|---|---|
| Vector mask clipping not applied | Zone subpath data is available as metadata but mask is not rendered into pixels |
| Blend mode rendering not implemented | `flatten_layers_rgba` composites as simple alpha-over; no multiply/screen/etc. |
| Non-8-bit RGB color modes | 16-bit and 32-bit PSDs won't parse correctly |
| Layer visibility mutation | Cannot toggle visibility at runtime (animation sprite workflow) |
| No layer composite for groups | No built-in "merge visible children of a group" |

---

## 3. Gap Analysis

### 3.1 GREEN - Fully Supported in Rust

| psd-to-json Feature | Rust Coverage |
|---|---|
| Layer naming convention parser | Pure string parsing -- no PSD dependency |
| Point category (center coords) | `layer_left/top/right/bottom` provides bbox |
| Zone category (bbox fallback) | Layer bounds available |
| Zone category (vector paths) | `vector_mask()` returns `Subpath`/`BezierKnot` with normalized coords |
| Group hierarchy + recursion | `groups()`, `parent_id()`, `get_group_sub_layers()` |
| Layer opacity capture | `opacity()` returns 0-255 |
| Blend mode capture | `blend_mode()` returns `BlendMode` enum |
| Raster mask metadata | `mask()` with full bounding box |
| Raster mask pixel export | `mask_pixels()` returns grayscale buffer |
| Depth/z-order tracking | Layer iteration order is consistent |
| Top-level PSD metadata | `width()`, `height()`, color mode, etc. |
| `--metadata-only` mode | Just skip image-writing code paths |
| JSON generation | `serde_json` is standard in Rust |
| Config file loading | `serde_json` deserialization |

### 3.2 YELLOW - Achievable with Workarounds

| psd-to-json Feature | Gap | Workaround |
|---|---|---|
| **Basic sprite export** | No built-in PNG encoder in psd crate | Use `image` crate to encode `rgba()` / `composite_rgba()` into PNG |
| **Group sprite merge** | No "merge group children" API | Manually composite child layers onto a canvas using the `image` crate (alpha-over blending) |
| **Spritesheet creation** | Not a PSD-parsing concern | Implement grid layout with `image` crate |
| **Animation spritesheet** | Needs visibility toggling per child | Child layers can be iterated and composited individually; visibility flag is readable even if not mutable |
| **Atlas / texture packing** | Python uses a `TexturePacker` lib | Implement a simple shelf/row packer or use `texture_packer` crate |
| **Tile slicing** | Not a PSD concern | Crop regions from composited image using `image` crate |
| **Tile JPG export** | Need JPEG encoder | `image` crate supports JPEG encoding |
| **Tile scaling** | Need image resize | `image` crate has `resize()` with Lanczos filter |
| **PNG optimization** | Python shells out to `pngquant` | Use `imagequant` crate (pure Rust libimagequant binding) -- works on iOS/iPad |
| **File watching** | Python polls mtime | Use `notify` crate (cross-platform file watcher) |
| **Vector mask coords -> pixels** | Normalized 0-1 coords from crate | Multiply by PSD width/height (same as Python does) |

### 3.3 RED - Genuine Gaps / Limitations

| psd-to-json Feature | Gap | Severity | Notes |
|---|---|---|---|
| **Blend mode compositing** | `flatten_layers_rgba` only does alpha-over; no multiply, screen, overlay, etc. | **Medium** | Affects visual correctness of merged group sprites. Most game assets use Normal blend mode. Document this limitation. |
| **Layer visibility mutation** | Cannot set `layer.visible = true/false` at runtime | **Low** | Animation sprites sort by numeric name and composite each child; we just iterate children directly -- visibility flag is not needed if we process all children in the group. |
| **Non-RGB color modes** | CMYK, Lab, Grayscale PSDs | **Low** | Game asset PSDs are almost always RGB. Log a warning and skip. |
| **16/32-bit depth** | Only 8-bit channels | **Low** | Same reasoning -- game assets are 8-bit. |
| **Smart objects** | Not parsed by psd crate | **None** | Python psd-to-json doesn't handle these either. |
| **Adjustment layers** | Not parsed | **None** | Python psd-to-json ignores these (no pipe-delimited name). |
| **Text layer content** | Not parsed | **None** | Python psd-to-json ignores text content (layers must be pipe-named). |

---

## 4. Tauri 2.0 / iPad Deployment Considerations

| Concern | Assessment |
|---|---|
| **File system access** | Tauri 2.0 supports scoped filesystem access on iOS. Config + PSD file reading is fine within the app sandbox. |
| **pngquant binary** | Cannot shell out to `pngquant` on iPad. Use `imagequant` crate (pure Rust, compiles to iOS ARM64). |
| **File watching** | `notify` crate works on macOS/iOS via `kqueue`. Consider using Tauri's file-system events API instead. |
| **Memory** | Large PSDs (e.g., 8192x8192) can consume significant RAM when decompressed to RGBA. iPad has constrained memory. Consider processing layers individually rather than holding all pixel data simultaneously. |
| **Thread safety** | Image processing (tile slicing, PNG encoding) benefits from parallelism. Tauri 2.0 supports Rust-side threading. Use `rayon` for parallel tile/sprite processing. |
| **JPEG encoding** | The `image` crate's JPEG encoder is pure Rust -- no system dependency issues. |
| **Output paths** | iPad file paths differ from desktop. Use Tauri's `app_data_dir` or let the frontend specify output paths. |
| **Large tile sets** | Generating hundreds of tile PNGs could be slow. Consider background processing with progress reporting via Tauri events. |

---

## 5. Recommended Architecture

```
psd-to-json-rust/
  Cargo.toml
  src/
    lib.rs              # Public API: process_psd(config) -> PsdOutput
    config.rs           # Config file deserialization
    parser.rs           # Layer name parser (pipes + attributes)
    processor.rs        # Main PSD processing loop
    types/
      mod.rs
      point.rs          # Point category handler
      zone.rs           # Zone category handler
      sprite.rs         # Sprite trait + factory
      sprite_basic.rs   # Basic sprite
      sprite_sheet.rs   # Spritesheet sprite
      sprite_animation.rs  # Animation sprite
      sprite_atlas.rs   # Atlas sprite
      tiles.rs          # Tile slicer
    export/
      mod.rs
      image_export.rs   # PNG/JPG encoding via image crate
      optimize.rs       # PNG optimization via imagequant
      mask_export.rs    # Mask PNG export
    output.rs           # JSON serialization with serde
  tests/
    integration.rs
```

### Key Dependencies

```toml
[dependencies]
psd = { git = "https://github.com/laffan/psd", branch = "master" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
image = "0.25"
imagequant = "4"          # Pure Rust PNG optimization (replaces pngquant CLI)
lodepng = "3"             # For writing optimized PNGs from imagequant output
rayon = "1"               # Parallel processing for tiles
notify = "7"              # File watching (optional, desktop only)
```

---

## 6. Implementation Priority

1. **Config + name parser + JSON output** -- no image processing, closest to `--metadata-only`
2. **Basic sprite export** -- single layer PNG
3. **Group sprite merge** -- composite children into one PNG
4. **Mask export** -- raster mask as grayscale PNG
5. **Tile slicing** -- PNG and JPG at base + scaled sizes
6. **Spritesheet + animation** -- grid layout
7. **Atlas** -- texture packing
8. **PNG optimization** -- imagequant integration
9. **File watching** -- notify crate (desktop only, skip for Tauri/iPad)
