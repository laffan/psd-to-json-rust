use image::RgbaImage;
use psd::BlendMode;

/// Composite `src` onto `dst` at position (dx, dy) using the given blend mode.
///
/// Implements the W3C compositing formula:
///   Cs' = (1 - αb) × Cs + αb × B(Cb, Cs)
///   co  = Cs' × αs + Cb × αb × (1 - αs)
///   αo  = αs + αb × (1 - αs)
///
/// where B(Cb, Cs) is the per-channel blend function selected by `mode`.
pub fn composite(dst: &mut RgbaImage, src: &RgbaImage, dx: i32, dy: i32, mode: BlendMode) {
    let (sw, sh) = src.dimensions();
    let dw = dst.width();
    let dh = dst.height();
    let blend_fn = blend_function(mode);

    for sy in 0..sh {
        let ty = dy + sy as i32;
        if ty < 0 || ty as u32 >= dh {
            continue;
        }
        for sx in 0..sw {
            let tx = dx + sx as i32;
            if tx < 0 || tx as u32 >= dw {
                continue;
            }
            let tx = tx as u32;
            let ty = ty as u32;

            let src_px = src.get_pixel(sx, sy);
            if src_px[3] == 0 {
                continue;
            }

            let dst_px = dst.get_pixel(tx, ty);
            let sa = src_px[3] as f32 / 255.0;
            let da = dst_px[3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);

            if out_a == 0.0 {
                continue;
            }

            let r = blend_channel(src_px[0], dst_px[0], sa, da, out_a, blend_fn);
            let g = blend_channel(src_px[1], dst_px[1], sa, da, out_a, blend_fn);
            let b = blend_channel(src_px[2], dst_px[2], sa, da, out_a, blend_fn);
            let a = (out_a * 255.0) as u8;

            dst.put_pixel(tx, ty, image::Rgba([r, g, b, a]));
        }
    }
}

/// Apply the W3C compositing formula for a single channel.
#[inline(always)]
fn blend_channel(
    src: u8,
    dst: u8,
    sa: f32,
    da: f32,
    out_a: f32,
    blend_fn: fn(f32, f32) -> f32,
) -> u8 {
    let cs = src as f32 / 255.0;
    let cb = dst as f32 / 255.0;
    // Blended source: mix original source with the blend result weighted by backdrop alpha.
    let cs_blend = (1.0 - da) * cs + da * blend_fn(cb, cs);
    // Pre-multiplied composite.
    let co = cs_blend * sa + cb * da * (1.0 - sa);
    // Un-premultiply.
    (((co / out_a) * 255.0).round().clamp(0.0, 255.0)) as u8
}

// ---------------------------------------------------------------------------
// Blend functions: B(Cb, Cs)
//   Cb = backdrop (destination), Cs = source
//   All values in 0.0..1.0
// ---------------------------------------------------------------------------

fn blend_function(mode: BlendMode) -> fn(f32, f32) -> f32 {
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => normal,
        // Darken group
        BlendMode::Darken => darken,
        BlendMode::Multiply => multiply,
        BlendMode::ColorBurn => color_burn,
        BlendMode::LinearBurn => linear_burn,
        // Lighten group
        BlendMode::Lighten => lighten,
        BlendMode::Screen => screen,
        BlendMode::ColorDodge => color_dodge,
        BlendMode::LinearDodge => linear_dodge,
        // Contrast group
        BlendMode::Overlay => overlay,
        BlendMode::SoftLight => soft_light,
        BlendMode::HardLight => hard_light,
        BlendMode::VividLight => vivid_light,
        BlendMode::LinearLight => linear_light,
        BlendMode::PinLight => pin_light,
        BlendMode::HardMix => hard_mix,
        // Inversion group
        BlendMode::Difference => difference,
        BlendMode::Exclusion => exclusion,
        BlendMode::Subtract => subtract,
        BlendMode::Divide => divide,
        // Unsupported — fall back to Normal.
        // Dissolve, DarkerColor, LighterColor need whole-pixel or
        // multi-channel logic. Hue, Saturation, Color, Luminosity
        // need HSL conversion. All are rare in game assets.
        _ => normal,
    }
}

// --- Normal ---

#[inline(always)]
fn normal(_cb: f32, cs: f32) -> f32 {
    cs
}

// --- Darken group ---

#[inline(always)]
fn darken(cb: f32, cs: f32) -> f32 {
    cb.min(cs)
}

#[inline(always)]
fn multiply(cb: f32, cs: f32) -> f32 {
    cb * cs
}

#[inline(always)]
fn color_burn(cb: f32, cs: f32) -> f32 {
    if cb == 1.0 {
        1.0
    } else {
        (1.0 - (1.0 - cs) / cb).max(0.0)
    }
}

#[inline(always)]
fn linear_burn(cb: f32, cs: f32) -> f32 {
    (cb + cs - 1.0).max(0.0)
}

// --- Lighten group ---

#[inline(always)]
fn lighten(cb: f32, cs: f32) -> f32 {
    cb.max(cs)
}

#[inline(always)]
fn screen(cb: f32, cs: f32) -> f32 {
    cb + cs - cb * cs
}

#[inline(always)]
fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cb == 0.0 {
        0.0
    } else if cs == 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

#[inline(always)]
fn linear_dodge(cb: f32, cs: f32) -> f32 {
    (cb + cs).min(1.0)
}

// --- Contrast group ---

#[inline(always)]
fn overlay(cb: f32, cs: f32) -> f32 {
    // Overlay is HardLight with swapped arguments.
    hard_light(cs, cb)
}

fn soft_light(cb: f32, cs: f32) -> f32 {
    let d = if cb <= 0.25 {
        ((16.0 * cb - 12.0) * cb + 4.0) * cb
    } else {
        cb.sqrt()
    };
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

#[inline(always)]
fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        multiply(cb, 2.0 * cs)
    } else {
        screen(cb, 2.0 * cs - 1.0)
    }
}

#[inline(always)]
fn vivid_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        color_burn(cb, 2.0 * cs)
    } else {
        color_dodge(cb, 2.0 * cs - 1.0)
    }
}

#[inline(always)]
fn linear_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        linear_burn(cb, 2.0 * cs)
    } else {
        linear_dodge(cb, 2.0 * cs - 1.0)
    }
}

#[inline(always)]
fn pin_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        darken(cb, 2.0 * cs)
    } else {
        lighten(cb, 2.0 * cs - 1.0)
    }
}

#[inline(always)]
fn hard_mix(cb: f32, cs: f32) -> f32 {
    if vivid_light(cb, cs) < 0.5 { 0.0 } else { 1.0 }
}

// --- Inversion group ---

#[inline(always)]
fn difference(cb: f32, cs: f32) -> f32 {
    (cb - cs).abs()
}

#[inline(always)]
fn exclusion(cb: f32, cs: f32) -> f32 {
    cb + cs - 2.0 * cb * cs
}

#[inline(always)]
fn subtract(cb: f32, cs: f32) -> f32 {
    (cb - cs).max(0.0)
}

#[inline(always)]
fn divide(cb: f32, cs: f32) -> f32 {
    if cs == 0.0 { cb } else { (cb / cs).min(1.0) }
}
