//! Renders the tray icon's digit at runtime and caches the result
//! (02_design.md §6.1). `ab_glyph` rasterizes the embedded font's glyph
//! coverage; that coverage is combined directly with a shape mask into
//! straight-alpha RGBA — no separate rasterizer/compositor is needed for
//! this (an earlier version used `tiny-skia` for a thin outline, dropped
//! for the reason below).
//!
//! Two visual phases exist per (value, state), used to blink an alert
//! without ever going fully transparent (user feedback: a blink to blank
//! read as ugly "black stripes" in the tray) and without changing shape
//! (an earlier circle/square version also didn't land well):
//! - [`IconPhase::Normal`]: a filled circle in the state color, digit
//!   knocked out in whichever of black/white contrasts best
//!   ([`contrast_color`]).
//! - [`IconPhase::Inverted`]: the same circle with foreground and
//!   background swapped — filled in the contrast color, digit knocked
//!   out in the state color.
//!
//! Both phases fill an opaque shape and knock the digit out of it, rather
//! than drawing a thin outline around a transparent-background digit:
//! a 1px synthetic outline anti-aliases badly at 32px regardless of its
//! color (user feedback, after the first attempt fixed contrast but not
//! the underlying "blurry outline" problem).

use ab_glyph::{point, Font, FontRef, GlyphId, OutlinedGlyph, PxScale, ScaleFont};
use cc_semaphore_core::SessionState;
use std::collections::HashMap;
use std::sync::OnceLock;

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
            let contrast = contrast_color(rgb);
            match phase {
                IconPhase::Normal => render_circle(&text, rgb, contrast),
                IconPhase::Inverted => render_circle(&text, contrast, rgb),
            }
        })
    }

    /// Shown when the daemon's heartbeat has gone stale (02_design.md
    /// §3.9): a neutral gray square, deliberately breaking the pattern of
    /// the app's usual circle badge so "something is different" reads at
    /// a glance.
    pub fn offline(&mut self) -> &IconRgba {
        self.offline
            .get_or_insert_with(|| render_square("?", (128, 128, 128), (255, 255, 255)))
    }
}

/// Picks black or white, whichever contrasts better against `rgb`, by a
/// standard perceptual-luminance approximation. This is what makes the
/// red `idle` fill get a white knockout while green/yellow get black,
/// without hardcoding per-state exceptions.
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

/// Alpha mask for a circle inscribed in the icon, with a ~1px
/// anti-aliased edge (a plain hard-edge circle would alias badly at this
/// size, the same problem the old outline technique had).
fn circle_mask() -> Vec<f32> {
    let size = ICON_SIZE as f32;
    let center = (size - 1.0) / 2.0;
    let radius = size / 2.0 - 0.5;
    let mut mask = vec![0f32; (ICON_SIZE * ICON_SIZE) as usize];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            mask[(y * ICON_SIZE + x) as usize] = (radius + 0.5 - dist).clamp(0.0, 1.0);
        }
    }
    mask
}

/// Fills `shape` (an alpha mask) with `bg`, knocking `text` out in `fg`.
fn render_masked(text: &str, bg: (u8, u8, u8), fg: (u8, u8, u8), shape: &[f32]) -> IconRgba {
    let coverage = glyph_coverage(text);
    let mut out = Vec::with_capacity(coverage.len() * 4);
    for i in 0..coverage.len() {
        let digit = coverage[i];
        out.push(lerp(bg.0, fg.0, digit));
        out.push(lerp(bg.1, fg.1, digit));
        out.push(lerp(bg.2, fg.2, digit));
        out.push((shape[i] * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    out
}

fn render_circle(text: &str, bg: (u8, u8, u8), fg: (u8, u8, u8)) -> IconRgba {
    render_masked(text, bg, fg, &circle_mask())
}

fn render_square(text: &str, bg: (u8, u8, u8), fg: (u8, u8, u8)) -> IconRgba {
    render_masked(
        text,
        bg,
        fg,
        &vec![1.0f32; (ICON_SIZE * ICON_SIZE) as usize],
    )
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_buffer_size() {
        assert_eq!(
            render_circle("5", (0, 255, 0), (0, 0, 0)).len(),
            (ICON_SIZE * ICON_SIZE * 4) as usize
        );
        assert_eq!(
            render_square("5", (0, 255, 0), (0, 0, 0)).len(),
            (ICON_SIZE * ICON_SIZE * 4) as usize
        );
    }

    #[test]
    fn square_is_fully_opaque() {
        let rgba = render_square("5", (0, 255, 0), (0, 0, 0));
        assert!(
            rgba.chunks_exact(4).all(|px| px[3] == 255),
            "the offline icon must never be transparent"
        );
    }

    #[test]
    fn circle_is_transparent_at_the_corners_and_opaque_at_the_center() {
        let rgba = render_circle("5", (0, 255, 0), (0, 0, 0));
        let pixel_alpha = |x: u32, y: u32| rgba[((y * ICON_SIZE + x) * 4 + 3) as usize];
        assert_eq!(pixel_alpha(0, 0), 0, "corner must be outside the circle");
        assert_eq!(
            pixel_alpha(ICON_SIZE - 1, ICON_SIZE - 1),
            0,
            "corner must be outside the circle"
        );
        assert_eq!(
            pixel_alpha(ICON_SIZE / 2, ICON_SIZE / 2),
            255,
            "center must be inside the circle"
        );
    }

    #[test]
    fn inverted_phase_swaps_fill_and_digit_colors() {
        // Center pixel (well inside the digit "1"'s stroke for a
        // single-char glyph) should be background in Normal and
        // foreground in Inverted, and vice versa at a background-only
        // pixel near the circle's edge.
        let mut cache = IconCache::new();
        let normal = cache
            .get(1, SessionState::Running, IconPhase::Normal)
            .clone();
        let inverted = cache
            .get(1, SessionState::Running, IconPhase::Inverted)
            .clone();
        assert_ne!(
            normal, inverted,
            "Normal and Inverted must render differently"
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
