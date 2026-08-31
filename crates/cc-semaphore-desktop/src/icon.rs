//! Renders the tray icon's digit at runtime and caches the result
//! (02_design.md §6.1). `ab_glyph` rasterizes the embedded font's glyph
//! coverage; `tiny-skia`'s `Pixmap` is the actual pixel canvas the
//! "normal" phase (fill + a 1px outline) is composited into.
//!
//! Two visual phases exist per (value, state), used to blink an alert
//! without ever going fully transparent (user feedback: a blink to blank
//! read as ugly "black stripes" in the tray):
//! - [`IconPhase::Normal`]: transparent background, digit filled in the
//!   state color, with a 1px outline.
//! - [`IconPhase::Inverted`]: an opaque block filled with the state color,
//!   digit knocked out — used as the alternate blink frame and nowhere
//!   else.
//!
//! Both the fill's outline (Normal) and the knockout color (Inverted) are
//! chosen by contrast against the fill color rather than a fixed dark
//! color: a fixed dark outline made the red `idle` digit hard to read
//! (dark-on-dark), per user feedback.

use ab_glyph::{point, Font, FontRef, GlyphId, OutlinedGlyph, PxScale, ScaleFont};
use cc_semaphore_core::SessionState;
use std::collections::HashMap;
use std::sync::OnceLock;
use tiny_skia::{Pixmap, PremultipliedColorU8};

pub const ICON_SIZE: u32 = 32;
/// Target span (in px) the widest dimension of the glyph should fill,
/// leaving a small margin inside the 32px icon (user feedback: digits
/// were too small — make them "as large as legibly possible").
const FIT_TARGET: f32 = 30.0;

/// Values are clamped into the cache key range here: 0..=99 map to
/// themselves, anything >=100 collapses to "9+" (02_design.md §6.1) so the
/// cache never grows past 100 numeric buckets + 1 overflow bucket, times 3
/// states, times 2 phases.
fn clamp_key(value: u32) -> u8 {
    value.min(100) as u8
}

fn key_text(key: u8) -> String {
    if key >= 100 {
        "9+".to_string()
    } else {
        key.to_string()
    }
}

static FONT_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSans-Bold.ttf");

fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("embedded font must parse"))
}

/// Straight-alpha RGBA bytes, `ICON_SIZE * ICON_SIZE * 4` long, row-major
/// top to bottom — the format `tauri::image::Image::new_owned` expects.
pub type IconRgba = Vec<u8>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconPhase {
    Normal,
    Inverted,
}

/// Caches rasterized icons by `(clamped value, state, phase)`. At most
/// 101 * 3 * 2 = 606 entries ever exist; in practice only a handful are
/// used.
#[derive(Default)]
pub struct IconCache {
    cache: HashMap<(u8, SessionState, IconPhase), IconRgba>,
    offline: Option<IconRgba>,
}

impl IconCache {
    pub fn new() -> Self {
        IconCache::default()
    }

    pub fn get(&mut self, value: u32, state: SessionState, phase: IconPhase) -> &IconRgba {
        let key = clamp_key(value);
        self.cache.entry((key, state, phase)).or_insert_with(|| {
            let text = key_text(key);
            let rgb = cc_semaphore_core::colors::rgb_for(state);
            match phase {
                IconPhase::Normal => render_normal(&text, rgb),
                IconPhase::Inverted => render_block(&text, rgb),
            }
        })
    }

    /// Shown when the daemon's heartbeat has gone stale (02_design.md
    /// §3.9): a neutral gray block distinct from every real state color.
    pub fn offline(&mut self) -> &IconRgba {
        self.offline
            .get_or_insert_with(|| render_block("?", (128, 128, 128)))
    }
}

/// Picks black or white, whichever contrasts better against `rgb`, by a
/// standard perceptual-luminance approximation. This is what makes the
/// red `idle` fill get a white outline/knockout while green/yellow get
/// black, without hardcoding per-state exceptions.
fn contrast_color(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let luminance = 0.2126 * rgb.0 as f32 + 0.7152 * rgb.1 as f32 + 0.0722 * rgb.2 as f32;
    if luminance > 140.0 {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

/// The font size that makes `text`'s widest dimension span `FIT_TARGET`
/// px, computed by measuring at a large reference size and scaling down —
/// robust to both single digits (height-bound) and "9+"/two-digit numbers
/// (width-bound).
fn fitted_scale(text: &str) -> PxScale {
    const PROBE: f32 = 100.0;
    let scaled = font().as_scaled(PxScale::from(PROBE));
    let mut width = 0.0f32;
    for ch in text.chars() {
        width += scaled.h_advance(scaled.glyph_id(ch));
    }
    let height = scaled.ascent() - scaled.descent();
    let factor = (FIT_TARGET / width).min(FIT_TARGET / height);
    PxScale::from(PROBE * factor)
}

/// Rasterizes `text`, auto-fit and centered, into a 32x32 coverage mask
/// (0.0 = uncovered, 1.0 = fully covered).
fn glyph_coverage(text: &str) -> Vec<f32> {
    let scale = fitted_scale(text);
    let scaled = font().as_scaled(scale);

    let mut glyphs = Vec::new();
    let mut caret = 0.0f32;
    for ch in text.chars() {
        let id: GlyphId = scaled.glyph_id(ch);
        glyphs.push(id.with_scale_and_position(scale, point(caret, 0.0)));
        caret += scaled.h_advance(id);
    }
    let total_width = caret;

    // Vertically center on the font's own ascent/descent rather than each
    // glyph's ink bounds, so digits with different heights (e.g. no
    // descender) still sit on a common baseline.
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let text_height = ascent - descent;
    let offset_x = ((ICON_SIZE as f32 - total_width) / 2.0).round() as i32;
    let offset_y = ((ICON_SIZE as f32 - text_height) / 2.0 + ascent).round() as i32;

    let outlined: Vec<OutlinedGlyph> = glyphs
        .into_iter()
        .filter_map(|g| font().outline_glyph(g))
        .collect();

    let mut coverage = vec![0f32; (ICON_SIZE * ICON_SIZE) as usize];
    for glyph in &outlined {
        let bounds = glyph.px_bounds();
        glyph.draw(|x, y, c| {
            let abs_x = bounds.min.x as i32 + x as i32 + offset_x;
            let abs_y = bounds.min.y as i32 + y as i32 + offset_y;
            if abs_x >= 0 && abs_x < ICON_SIZE as i32 && abs_y >= 0 && abs_y < ICON_SIZE as i32 {
                let idx = (abs_y as u32 * ICON_SIZE + abs_x as u32) as usize;
                coverage[idx] = coverage[idx].max(c.min(1.0));
            }
        });
    }
    coverage
}

/// Max coverage among each pixel's 8 neighbors (plus itself), giving a 1px
/// halo around the glyph fill to draw the outline from.
fn dilate(coverage: &[f32]) -> Vec<f32> {
    let size = ICON_SIZE as i32;
    let mut out = vec![0f32; coverage.len()];
    for y in 0..size {
        for x in 0..size {
            let mut m = 0f32;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && nx < size && ny >= 0 && ny < size {
                        m = m.max(coverage[(ny * size + nx) as usize]);
                    }
                }
            }
            out[(y * size + x) as usize] = m;
        }
    }
    out
}

/// Transparent background, digit filled with `rgb`, outlined 1px in
/// whichever of black/white contrasts best against `rgb`.
fn render_normal(text: &str, rgb: (u8, u8, u8)) -> IconRgba {
    let coverage = glyph_coverage(text);
    let outline_rgb = contrast_color(rgb);
    let outline_coverage = dilate(&coverage);

    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE).expect("32x32 is a valid pixmap size");
    let pixels = pixmap.pixels_mut();
    for i in 0..coverage.len() {
        pixels[i] = composite(outline_coverage[i], outline_rgb, coverage[i], rgb);
    }
    unpremultiply(pixmap.pixels())
}

/// An opaque `rgb`-filled square with the digit knocked out in whichever
/// of black/white contrasts best against `rgb` — e.g. a black "1" on a
/// solid yellow block, or a white "2" on solid red.
fn render_block(text: &str, rgb: (u8, u8, u8)) -> IconRgba {
    let coverage = glyph_coverage(text);
    let contrast = contrast_color(rgb);
    let mut out = Vec::with_capacity(coverage.len() * 4);
    for c in coverage {
        out.push(lerp(rgb.0, contrast.0, c));
        out.push(lerp(rgb.1, contrast.1, c));
        out.push(lerp(rgb.2, contrast.2, c));
        out.push(255);
    }
    out
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Straight-alpha "src-over-src-over-transparent" composite of the outline
/// layer under the fill layer, returned already premultiplied for storage
/// in a `tiny_skia::Pixmap`.
fn composite(
    outline_a: f32,
    outline_rgb: (u8, u8, u8),
    fill_a: f32,
    fill_rgb: (u8, u8, u8),
) -> PremultipliedColorU8 {
    let out_a = fill_a + outline_a * (1.0 - fill_a);
    if out_a <= 0.0 {
        return PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap();
    }
    let blend = |fill_c: u8, outline_c: u8| -> u8 {
        let straight =
            (fill_c as f32 * fill_a + outline_c as f32 * outline_a * (1.0 - fill_a)) / out_a;
        straight.round().clamp(0.0, 255.0) as u8
    };
    let r = blend(fill_rgb.0, outline_rgb.0);
    let g = blend(fill_rgb.1, outline_rgb.1);
    let b = blend(fill_rgb.2, outline_rgb.2);
    let a = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    // Premultiply for tiny_skia storage: straight*alpha/255, which is
    // always <= a by construction, satisfying PremultipliedColorU8's
    // invariant.
    let pm = |c: u8| ((c as u32 * a as u32) / 255) as u8;
    PremultipliedColorU8::from_rgba(pm(r), pm(g), pm(b), a)
        .expect("premultiplied channels are always <= alpha")
}

fn unpremultiply(pixels: &[PremultipliedColorU8]) -> IconRgba {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for p in pixels {
        let a = p.alpha();
        let straight = |c: u8| -> u8 {
            if a == 0 {
                0
            } else {
                ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
            }
        };
        out.push(straight(p.red()));
        out.push(straight(p.green()));
        out.push(straight(p.blue()));
        out.push(a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_buffer_size() {
        assert_eq!(
            render_normal("5", (0, 255, 0)).len(),
            (ICON_SIZE * ICON_SIZE * 4) as usize
        );
        assert_eq!(
            render_block("5", (0, 255, 0)).len(),
            (ICON_SIZE * ICON_SIZE * 4) as usize
        );
    }

    #[test]
    fn normal_renders_some_visible_pixels() {
        let rgba = render_normal("5", (0, 255, 0));
        let has_visible = rgba.chunks_exact(4).any(|px| px[3] > 0);
        assert!(has_visible, "expected at least some non-transparent pixels");
    }

    #[test]
    fn block_is_fully_opaque() {
        let rgba = render_block("5", (0, 255, 0));
        assert!(
            rgba.chunks_exact(4).all(|px| px[3] == 255),
            "the inverted phase must never be transparent"
        );
    }

    #[test]
    fn cache_reuses_the_same_buffer_for_repeated_lookups() {
        let mut cache = IconCache::new();
        let a = cache
            .get(3, SessionState::Running, IconPhase::Normal)
            .clone();
        let b = cache
            .get(3, SessionState::Running, IconPhase::Normal)
            .clone();
        assert_eq!(a, b);
        assert_eq!(cache.cache.len(), 1);
    }

    #[test]
    fn normal_and_inverted_are_cached_separately() {
        let mut cache = IconCache::new();
        cache.get(3, SessionState::Running, IconPhase::Normal);
        cache.get(3, SessionState::Running, IconPhase::Inverted);
        assert_eq!(cache.cache.len(), 2);
    }

    #[test]
    fn values_at_or_above_100_collapse_to_one_cache_bucket() {
        assert_eq!(clamp_key(100), 100);
        assert_eq!(clamp_key(9_999), 100);
        assert_eq!(key_text(100), "9+");
        assert_eq!(key_text(42), "42");
    }

    #[test]
    fn contrast_picks_black_for_bright_colors_and_white_for_dark() {
        assert_eq!(contrast_color((0x2e, 0xc2, 0x7e)), (0, 0, 0)); // running: green
        assert_eq!(contrast_color((0xf5, 0xc2, 0x11)), (0, 0, 0)); // waiting: yellow
        assert_eq!(contrast_color((0xe0, 0x1b, 0x24)), (255, 255, 255)); // idle: red
    }

    #[test]
    fn offline_icon_is_cached_across_calls() {
        let mut cache = IconCache::new();
        let a = cache.offline().clone();
        let b = cache.offline().clone();
        assert_eq!(a, b);
    }
}
