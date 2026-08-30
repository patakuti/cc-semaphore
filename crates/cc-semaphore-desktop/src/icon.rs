//! Renders the tray icon's digit at runtime and caches the result
//! (02_design.md §6.1). `ab_glyph` rasterizes the embedded font's glyph
//! coverage; `tiny-skia`'s `Pixmap` is the actual pixel canvas the glyphs
//! (fill + a 1px dark outline for legibility) are composited into.

use ab_glyph::{point, Font, FontRef, Glyph, GlyphId, OutlinedGlyph, PxScale, ScaleFont};
use cc_semaphore_core::SessionState;
use std::collections::HashMap;
use std::sync::OnceLock;
use tiny_skia::{Pixmap, PremultipliedColorU8};

pub const ICON_SIZE: u32 = 32;
const FONT_SCALE: f32 = 24.0;
const OUTLINE_RGB: (u8, u8, u8) = (20, 20, 20);

/// Values are clamped into the cache key range here: 0..=99 map to
/// themselves, anything >=100 collapses to "9+" (02_design.md §6.1) so the
/// cache never grows past 100 numeric buckets + 1 overflow bucket, times 3
/// states.
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

/// Caches rasterized icons by `(clamped value, state)`. At most
/// 101 * 3 = 303 entries ever exist; in practice only a handful are used.
#[derive(Default)]
pub struct IconCache {
    cache: HashMap<(u8, SessionState), IconRgba>,
}

impl IconCache {
    pub fn new() -> Self {
        IconCache::default()
    }

    /// Blank (fully transparent) icon, used for the blink "off" phase.
    pub fn blank(&self) -> IconRgba {
        vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize]
    }

    pub fn get(&mut self, value: u32, state: SessionState) -> &IconRgba {
        let key = clamp_key(value);
        self.cache
            .entry((key, state))
            .or_insert_with(|| render(&key_text(key), cc_semaphore_core::colors::rgb_for(state)))
    }
}

/// Lays out `text` (1-2 chars) centered in a 32x32 canvas, filled with
/// `rgb` and outlined 1px in a dark color for legibility against any tray
/// background.
fn render(text: &str, rgb: (u8, u8, u8)) -> IconRgba {
    let scaled = font().as_scaled(PxScale::from(FONT_SCALE));

    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut caret = 0.0f32;
    for ch in text.chars() {
        let id: GlyphId = scaled.glyph_id(ch);
        glyphs.push(id.with_scale_and_position(scaled.scale(), point(caret, 0.0)));
        caret += scaled.h_advance(id);
    }
    let total_width = caret;

    let outlined: Vec<OutlinedGlyph> = glyphs
        .into_iter()
        .filter_map(|g| font().outline_glyph(g))
        .collect();

    // Vertically center on the font's own ascent/descent rather than each
    // glyph's ink bounds, so digits with different heights (e.g. no
    // descender) still sit on a common baseline.
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let text_height = ascent - descent;
    let offset_x = ((ICON_SIZE as f32 - total_width) / 2.0).round() as i32;
    let offset_y = ((ICON_SIZE as f32 - text_height) / 2.0 + ascent).round() as i32;

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

    let outline_coverage = dilate(&coverage);

    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE).expect("32x32 is a valid pixmap size");
    let pixels = pixmap.pixels_mut();
    for i in 0..coverage.len() {
        pixels[i] = composite(outline_coverage[i], OUTLINE_RGB, coverage[i], rgb);
    }

    unpremultiply(pixmap.pixels())
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
        let rgba = render("5", (0, 255, 0));
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    #[test]
    fn renders_some_visible_pixels() {
        let rgba = render("5", (0, 255, 0));
        let has_visible = rgba.chunks_exact(4).any(|px| px[3] > 0);
        assert!(has_visible, "expected at least some non-transparent pixels");
    }

    #[test]
    fn cache_reuses_the_same_buffer_for_repeated_lookups() {
        let mut cache = IconCache::new();
        let a = cache.get(3, SessionState::Running).clone();
        let b = cache.get(3, SessionState::Running).clone();
        assert_eq!(a, b);
        assert_eq!(cache.cache.len(), 1);
    }

    #[test]
    fn values_at_or_above_100_collapse_to_one_cache_bucket() {
        assert_eq!(clamp_key(100), 100);
        assert_eq!(clamp_key(9_999), 100);
        assert_eq!(key_text(100), "9+");
        assert_eq!(key_text(42), "42");
    }

    #[test]
    fn blank_icon_is_fully_transparent() {
        let cache = IconCache::new();
        let rgba = cache.blank();
        assert!(rgba.iter().all(|&b| b == 0));
    }
}
