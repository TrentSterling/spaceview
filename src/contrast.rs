//! Surface and compositing protection adapted from Trent's Trontop contrast pass.
//! Palette intent stays intact; text-bearing surfaces and ink are corrected.
use crate::color;
use eframe::egui::Color32;
use std::sync::LazyLock;

static LINEAR: LazyLock<[f32; 256]> =
    LazyLock::new(|| std::array::from_fn(|i| color::srgb_to_linear(i as f32 / 255.0)));

pub fn rgb(c: Color32) -> color::Rgb {
    [c.r(), c.g(), c.b()]
}
fn luminance(c: Color32) -> f32 {
    0.2126 * LINEAR[c.r() as usize]
        + 0.7152 * LINEAR[c.g() as usize]
        + 0.0722 * LINEAR[c.b() as usize]
}
pub fn ratio(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
pub fn mix(a: Color32, b: Color32, amount: f32) -> Color32 {
    let c = color::mix_colors(rgb(a), rgb(b), amount);
    Color32::from_rgb(c[0], c[1], c[2])
}
pub fn in_envelope(c: Color32, dark: bool) -> bool {
    if dark {
        luminance(c) <= luminance(Color32::from_gray(64))
    } else {
        0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32 >= 205.0
    }
}
fn toward(c: Color32, anchor: Color32, safe: impl Fn(Color32) -> bool) -> Color32 {
    if safe(c) {
        return c;
    }
    let (mut low, mut high) = (0.0, 1.0);
    let mut result = anchor;
    for _ in 0..12 {
        let amount = (low + high) * 0.5;
        let candidate = mix(c, anchor, amount);
        if safe(candidate) {
            high = amount;
            result = candidate;
        } else {
            low = amount;
        }
    }
    result
}
pub fn surface(c: Color32, dark: bool) -> Color32 {
    toward(
        c.to_opaque(),
        if dark { Color32::BLACK } else { Color32::WHITE },
        |c| in_envelope(c, dark),
    )
}
pub fn backdrop(raw: Color32, panel: Color32, alpha: u8, dark: bool) -> Color32 {
    toward(
        raw,
        if dark { Color32::BLACK } else { Color32::WHITE },
        |c| in_envelope(mix(c, panel, alpha as f32 / 255.0), dark),
    )
}
pub fn ink(preferred: Color32, dark: bool) -> Color32 {
    // A small guard beyond the surface envelope covers RGB/TRC differences
    // between the ratio metric and APCA, plus framebuffer quantization.
    let worst = Color32::from_gray(if dark { 76 } else { 198 });
    let anchor = if dark { Color32::WHITE } else { Color32::BLACK };
    toward(preferred, anchor, |c| {
        ratio(c, worst) >= 7.0 && color::apca_abs(rgb(c), rgb(worst)) >= color::LC_TEXT_MIN
    })
}
pub fn outline(preferred: Color32, dark: bool) -> Color32 {
    let worst = Color32::from_gray(if dark { 64 } else { 205 });
    toward(
        preferred,
        if dark { Color32::WHITE } else { Color32::BLACK },
        |c| ratio(c, worst) >= 3.0 && color::apca_abs(rgb(c), rgb(worst)) >= color::LC_NONTEXT,
    )
}
pub fn on_fill(fill: Color32) -> Color32 {
    let white = Color32::WHITE;
    let black = Color32::BLACK;
    // A small label must clear the ratio floor before APCA chooses its polarity.
    if ratio(white, fill) < 4.5 {
        black
    } else if ratio(black, fill) < 4.5 {
        white
    } else if color::apca_abs(rgb(white), rgb(fill)) > color::apca_abs(rgb(black), rgb(fill)) {
        white
    } else {
        black
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_color_keeps_small_text_and_button_outlines_readable() {
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    let raw = Color32::from_rgb(r, g, b);
                    for dark in [false, true] {
                        let bg = surface(raw, dark);
                        let text = ink(raw, dark);
                        assert!(ratio(text, bg) >= 7.0, "{text:?} on {bg:?}");
                        assert!(
                            color::apca_abs(rgb(text), rgb(bg)) >= color::LC_TEXT_MIN,
                            "dark={dark} raw={raw:?} text={text:?} bg={bg:?} Lc={}",
                            color::apca_abs(rgb(text), rgb(bg))
                        );
                        assert!(ratio(outline(raw, dark), bg) >= 3.0);
                        assert!(ratio(on_fill(raw), raw) >= 4.5);
                        assert_eq!(surface(bg, dark), bg);
                    }
                }
            }
        }
    }
    #[test]
    fn composited_gradient_is_safe_at_every_frost_and_intensity() {
        let colors = [
            Color32::WHITE,
            Color32::BLACK,
            Color32::YELLOW,
            Color32::from_rgb(180, 40, 255),
            Color32::from_rgb(25, 240, 180),
        ];
        for dark in [false, true] {
            let panel = surface(Color32::from_rgb(50, 40, 70), dark);
            let text = ink(Color32::from_gray(160), dark);
            for a in colors {
                for b in colors {
                    for alpha in (0..=255).step_by(17) {
                        for i in 0..=16 {
                            let raw = mix(a, b, i as f32 / 16.0);
                            let wash = backdrop(raw, panel, alpha, dark);
                            let actual = mix(wash, panel, alpha as f32 / 255.0);
                            assert!(ratio(text, actual) >= 7.0);
                            assert!(
                                color::apca_abs(rgb(text), rgb(actual)) >= color::LC_TEXT_MIN,
                                "dark={dark} text={text:?} actual={actual:?} alpha={alpha}"
                            );
                        }
                    }
                }
            }
        }
    }
}
