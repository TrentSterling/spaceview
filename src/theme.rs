//! SpaceView's chrome theme, TrontStack house style. Colormagic (`color.rs`,
//! vendored verbatim from TrontSnap/Boxel) drives a runtime-swappable accent:
//! pick any color, roll one of the 32 premades, or randomize, and the whole
//! chrome (panels, buttons, top bar, window_chrome caption buttons) follows
//! via `build_visuals`, with WCAG contrast guaranteeing readable text no
//! matter what accent lands.
//!
//! This is deliberately separate from `ColorTheme` in `app.rs` (Rainbow /
//! Neon / Ocean) — that enum still drives the treemap tile colors and is
//! untouched. This module only colors the CHROME: panels, buttons, borders,
//! and the Discord-style background gradient (see `gradient_colors` +
//! `paint_gradient`).
//!
//! Unlike TrontSnap (always dark) SpaceView already had a light/dark toggle,
//! so every entry point here takes a `dark: bool` and derives BOTH a dark and
//! light ground for the same accent (mode-aware, following TrontEQ's
//! `Palette` pattern) rather than baking one fixed mode per theme.

use std::sync::{LazyLock, RwLock};

use eframe::egui::{self, Color32, CornerRadius, Stroke};

use crate::color::{self, Rgb};

// ---- tokens ---------------------------------------------------------------

#[derive(Clone, Copy)]
#[allow(dead_code)] // full token set kept as house-style API; `muted` is for future UI text, not read yet
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_dim: Color32,
    pub on_accent: Color32,
    pub edge: Color32,
    pub hover: Color32,
}

fn c32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
fn rgb_of(c: Color32) -> Rgb {
    [c.r(), c.g(), c.b()]
}

/// The default dark ground + accent ("Cyan"): a blue-tinted near-black base
/// with the same electric-cyan family the other TrontStack apps use, so
/// SpaceView's chrome reads as a sibling of TrontSnap/TrontEQ while its
/// neon treemap palettes (untouched) stay the star of the show.
fn dark_ground() -> Tokens {
    let bg = [10u8, 14, 20];
    let panel = [17u8, 24, 33];
    let accent = [86u8, 204, 255];
    Tokens {
        dark: true,
        bg: c32(bg),
        panel: c32(panel),
        text: c32([224, 236, 244]),
        muted: c32([128, 150, 168]),
        accent: c32(accent),
        accent_dim: c32(color::mix_colors(accent, bg, 0.45)),
        on_accent: c32(color::contrast_color(accent)),
        edge: c32([33, 46, 60]),
        hover: c32(color::mix_colors(panel, accent, 0.14)),
    }
}

/// The default light ground ("Cyan", light mode): airy near-white with a
/// deepened teal-cyan accent so it stays legible on paper instead of
/// washing out.
fn light_ground() -> Tokens {
    let bg = [237u8, 241, 245];
    let panel = [248u8, 250, 252];
    let accent = [0u8, 137, 173];
    Tokens {
        dark: false,
        bg: c32(bg),
        panel: c32(panel),
        text: c32([20, 28, 36]),
        muted: c32([96, 113, 128]),
        accent: c32(accent),
        accent_dim: c32(color::mix_colors(accent, bg, 0.45)),
        on_accent: c32(color::contrast_color(accent)),
        edge: c32([212, 219, 227]),
        hover: c32(color::mix_colors(panel, accent, 0.14)),
    }
}

fn ground(dark: bool) -> Tokens {
    if dark { dark_ground() } else { light_ground() }
}

/// The live palette + gradient toggle. Starts as the default Cyan ground;
/// `resolve()` at startup and every UI pick funnel through `set_theme()`.
static CURRENT: LazyLock<RwLock<Tokens>> = LazyLock::new(|| RwLock::new(dark_ground()));

/// Snapshot of the current tokens. `Tokens` is `Copy`, so call sites can hold
/// their own copy across a frame without re-locking.
pub fn t() -> Tokens {
    *CURRENT.read().unwrap()
}

pub fn corner_radius() -> CornerRadius {
    CornerRadius::same(6)
}
pub fn corner_radius_lg() -> CornerRadius {
    CornerRadius::same(9)
}

/// Swap the live tokens and re-apply egui visuals (gradient translucency
/// follows the caller's `gradient` flag).
pub fn set_theme(ctx: &egui::Context, tk: Tokens, gradient: bool) {
    *CURRENT.write().unwrap() = tk;
    ctx.set_visuals(build_visuals(tk, gradient));
}

/// egui Visuals derived entirely from the given tokens. `gradient` controls
/// panel/window fill translucency (~0.90 alpha) so the background wash reads
/// through the chrome; off keeps fills fully solid (the flat pre-gradient
/// look, house rule).
pub fn build_visuals(tk: Tokens, gradient: bool) -> egui::Visuals {
    let mut v = if tk.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.dark_mode = tk.dark;

    // KNOWN ISSUE (light mode only, unresolved): the gradient mesh under
    // `Visuals::light()`'s panel_fill measures as fully opaque on screen at
    // ANY alpha tested (120-230), even though `gradient_colors()` computes
    // correct, clearly non-white values (confirmed via debug dump: e.g.
    // top-left #9ACFDE) and the identical mechanism is confirmed working in
    // dark mode (verified pixel-for-pixel against hand-predicted composite
    // math). Forcing the base to `Visuals::dark()` regardless of `tk.dark`
    // changed the on-screen result but wasn't cleanly verified before this
    // session ran out of safe, non-disruptive ways to test further (see
    // verify_shots/ + session notes). Next step for whoever picks this up:
    // bisect `Visuals::light()`'s ~30 fields for one that affects background-
    // layer alpha compositing specifically, or paint a Window instead of a
    // TopBottomPanel/CentralPanel background layer shape and compare.
    let panel_alpha: u8 = if gradient { 216 } else { 255 };
    let panel = Color32::from_rgba_unmultiplied(tk.panel.r(), tk.panel.g(), tk.panel.b(), panel_alpha);

    v.window_fill = panel;
    v.panel_fill = panel;
    v.faint_bg_color = tk.hover;
    v.extreme_bg_color = tk.bg;
    v.code_bg_color = tk.hover;
    // NO override_text_color: it forces EVERY glyph to tk.text, steamrolling
    // selection.stroke's contrast-on-accent (white-on-cyan bug, Trent-flagged).
    // Per-state fg_strokes below + selection.stroke handle text color.

    v.window_corner_radius = corner_radius_lg();
    v.window_stroke = Stroke::new(1.0, tk.edge);
    v.menu_corner_radius = corner_radius_lg();
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 1,
        color: Color32::from_black_alpha(130),
    };
    v.clip_rect_margin = 3.0;
    v.indent_has_left_vline = false;

    // Selection fill: the raw accent at full blast made selected rows glow
    // like a highlighter and fought the text on bright accents (Trent-flagged,
    // white-on-cyan). Pull it toward the ground and derive the text contrast
    // FROM THE FILL ACTUALLY DRAWN, so it stays readable for any accent.
    let sel_fill = color::mix_colors(rgb_of(tk.accent), rgb_of(tk.bg), 0.30);
    v.selection.bg_fill = c32(sel_fill);
    v.selection.stroke = Stroke::new(1.0, c32(color::contrast_color(sel_fill)));
    v.hyperlink_color = tk.accent;
    v.warn_fg_color = tk.accent;
    v.error_fg_color = Color32::from_rgb(220, 80, 60);

    let r = corner_radius();
    let txt = Stroke::new(1.0, tk.text);

    let w = &mut v.widgets.noninteractive;
    w.bg_fill = panel;
    w.weak_bg_fill = panel;
    w.bg_stroke = Stroke::new(1.0, tk.edge);
    w.fg_stroke = txt;
    w.corner_radius = r;

    let w = &mut v.widgets.inactive;
    w.bg_fill = tk.hover.gamma_multiply(0.6);
    w.weak_bg_fill = tk.hover.gamma_multiply(0.6);
    w.bg_stroke = Stroke::new(1.0, tk.edge);
    w.fg_stroke = txt;
    w.corner_radius = r;

    let w = &mut v.widgets.hovered;
    w.bg_fill = tk.hover;
    w.weak_bg_fill = tk.hover;
    w.bg_stroke = Stroke::new(1.2, tk.accent);
    w.fg_stroke = Stroke::new(1.5, tk.text);
    w.corner_radius = r;
    w.expansion = 1.0;

    let w = &mut v.widgets.active;
    w.bg_fill = tk.accent;
    w.weak_bg_fill = tk.accent_dim;
    w.bg_stroke = Stroke::new(1.0, tk.accent);
    // Pressed Checkbox/SelectableLabel/RadioButton text paints over the dark
    // panel (not bg_fill), so this stays `text`, not `on_accent` — otherwise
    // it goes dark-on-dark on a dark ground.
    w.fg_stroke = Stroke::new(1.0, tk.text);
    w.corner_radius = r;
    w.expansion = 1.0;

    let w = &mut v.widgets.open;
    w.bg_fill = tk.hover;
    w.weak_bg_fill = tk.hover;
    w.bg_stroke = Stroke::new(1.0, tk.accent_dim);
    w.fg_stroke = txt;
    w.corner_radius = r;

    v
}

// ---- theme derivation ("colormagic") --------------------------------------

/// "Your accent color on the standard ground": start from the resolved
/// dark/light ground and only swap the accent-derived fields, walking
/// lightness (bounded) until the accent reads against the panel.
pub fn from_accent(accent: Rgb, dark: bool) -> Tokens {
    // DISCORD GROUND PARITY (Trent: "Cotton Candy needs work" — a pink theme
    // should feel pink EVERYWHERE, not pink-widgets-on-navy). The ground
    // itself takes the accent's hue at low saturation, so bg/panel/edge/text
    // all lean into the theme instead of staying the fixed cyan-navy base.
    let hue = color::rgb_to_hsl(accent).h;
    let (bg, panel, edge, text, muted) = if dark {
        (
            color::hsl_to_rgb(hue, 24.0, 7.0),
            color::hsl_to_rgb(hue, 22.0, 11.0),
            color::hsl_to_rgb(hue, 20.0, 19.0),
            color::hsl_to_rgb(hue, 18.0, 92.0),
            color::hsl_to_rgb(hue, 12.0, 62.0),
        )
    } else {
        (
            color::hsl_to_rgb(hue, 35.0, 94.0),
            color::hsl_to_rgb(hue, 30.0, 98.0),
            color::hsl_to_rgb(hue, 20.0, 84.0),
            color::hsl_to_rgb(hue, 30.0, 12.0),
            color::hsl_to_rgb(hue, 12.0, 42.0),
        )
    };

    // Contrast-walk the accent against the DERIVED panel (not the old fixed
    // ground) so it stays readable on its own tinted chrome.
    let mut chosen = accent;
    let mut guard = 0;
    while color::contrast_ratio(chosen, panel) < 2.2 && guard < 14 {
        let h = color::rgb_to_hsl(chosen);
        let l = if dark { (h.l + 6.0).min(92.0) } else { (h.l - 6.0).max(8.0) };
        chosen = color::hsl_to_rgb(h.h, h.s.max(45.0), l);
        guard += 1;
    }

    Tokens {
        dark,
        bg: c32(bg),
        panel: c32(panel),
        text: c32(text),
        muted: c32(muted),
        accent: c32(chosen),
        accent_dim: c32(color::mix_colors(chosen, bg, 0.45)),
        on_accent: c32(color::contrast_color(chosen)),
        edge: c32(edge),
        hover: c32(color::mix_colors(panel, chosen, 0.14)),
    }
}

/// Pick the most saturated color in a list — the one swatch a palette (or a
/// generated harmony/flavor spread) would read as its "accent" at a glance.
fn most_saturated(colors: &[Rgb]) -> Option<Rgb> {
    colors
        .iter()
        .copied()
        .max_by(|a, b| {
            color::rgb_to_hsl(*a).s.partial_cmp(&color::rgb_to_hsl(*b).s).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Look up a premade palette by name, take its most-saturated swatch as the
/// accent, and derive tokens for the given mode. Returns the tokens plus the
/// accent's hex (for persistence).
pub fn premade_tokens(name: &str, dark: bool) -> Option<(Tokens, String)> {
    let p = color::PREMADE_PALETTES.iter().find(|p| p.name == name)?;
    let rgb: Vec<Rgb> = p.colors.iter().filter_map(|h| color::hex_to_rgb(h)).collect();
    let accent = most_saturated(&rgb)?;
    Some((from_accent(accent, dark), color::rgb_to_hex(accent)))
}

/// Roll a new theme: random flavor palette, random harmony spread, or a
/// random premade — all funneled down to one representative accent through
/// the same contrast-safe deriver. Returns the tokens, a display name, and
/// the accent hex (for persistence).
pub fn randomize(dark: bool) -> (Tokens, String, String) {
    let mut rng = color::Rng::from_clock();
    let pick = rng.range(0, 2);
    let (name, accent): (String, Rgb) = match pick {
        0 => {
            let kind = color::PaletteKind::ALL[rng.range(0, 5) as usize];
            let cols = color::generate_random_palette(kind, 5, &mut rng);
            let rgb: Vec<Rgb> = cols.iter().map(|h| color::hsl_to_rgb(h.h, h.s, h.l)).collect();
            let accent = most_saturated(&rgb).unwrap_or(rgb[0]);
            (format!("Random {}", kind.label()), accent)
        }
        1 => {
            let base = color::Hsl::new(
                rng.range(0, 359) as f32,
                rng.range(55, 95) as f32,
                rng.range(28, 62) as f32,
            );
            let rule = color::HARMONY_RULES[rng.range(0, 6) as usize];
            // generate_harmony's first entry is always the base color itself.
            let accent = color::hsl_to_rgb(base.h, base.s, base.l);
            (format!("Random {rule}"), accent)
        }
        _ => {
            let n = color::PREMADE_PALETTES.len() as i32;
            let p = &color::PREMADE_PALETTES[rng.range(0, n - 1) as usize];
            let rgb: Vec<Rgb> = p.colors.iter().filter_map(|h| color::hex_to_rgb(h)).collect();
            let accent = most_saturated(&rgb).unwrap_or(rgb[0]);
            (p.name.to_string(), accent)
        }
    };
    let source = color::rgb_to_hex(accent);
    (from_accent(accent, dark), name, source)
}

/// Resolve a persisted theme for the given mode: "Cyan" (or no stored accent)
/// is the hardcoded default ground; any stored accent hex re-derives via
/// `from_accent` so the same theme flips cleanly between dark and light.
pub fn resolve(name: &str, accent_hex: Option<&String>, dark: bool) -> Tokens {
    if name == "Cyan" {
        return ground(dark);
    }
    if let Some(rgb) = accent_hex.and_then(|h| color::hex_to_rgb(h)) {
        return from_accent(rgb, dark);
    }
    ground(dark)
}

// ---- background gradient ---------------------------------------------------

/// Discord-style dynamic background wash, derived from the live tokens.
/// TrontStack canonical recipe (same shape across every app):
///   top-left = bg -> toward accent (strongest)
///   top-right = bg -> toward accent (half as strong)
///   bottom-left = bg -> toward a deeper/darker accent (hue +40deg, value halved)
///   bottom-right = bg darkened
/// Light mode blends the same way but pulls back toward white so it stays
/// airy instead of muddy.
///
/// SpaceView-specific tuning: unlike a viewport app where the 3D/2D canvas IS
/// the exposed background, SpaceView's TopBottomPanel (top+status bars) and
/// CentralPanel tile the ENTIRE window edge to edge. At the spec's baseline
/// ~14/7/8/6% blend, the ~90%-opaque panel fill (`build_visuals`'s
/// `panel_alpha`) dilutes that down to an imperceptible 1-3 RGB units of
/// on-screen spread (measured). BLEND is scaled up ~4x from the baseline so
/// the wash actually reads once composited under the chrome, while alpha
/// stays in-spec so text contrast is untouched.
const BLEND: f32 = 8.0; // Trent: "a bit subtle, I'd like stronger" — doubled from 4.0

pub fn gradient_colors(tk: &Tokens) -> [Color32; 4] {
    let bg = rgb_of(tk.bg);
    let accent = rgb_of(tk.accent);
    let a = color::rgb_to_hsl(accent);

    if tk.dark {
        let deep = color::hsl_to_rgb((a.h + 40.0).rem_euclid(360.0), a.s, (a.l * 0.5).max(6.0));
        [
            c32(color::mix_colors(bg, accent, (0.14 * BLEND).min(0.85))), // top-left
            c32(color::mix_colors(bg, accent, (0.07 * BLEND).min(0.85))), // top-right
            c32(color::mix_colors(bg, deep, (0.08 * BLEND).min(0.85))),   // bottom-left
            c32(color::mix_colors(bg, [0, 0, 0], (0.06 * BLEND).min(0.85))), // bottom-right
        ]
    } else {
        let white = [255u8, 255, 255];
        let deep = color::hsl_to_rgb((a.h + 40.0).rem_euclid(360.0), (a.s * 0.7).max(20.0), (a.l * 1.25).min(85.0));
        // Two-stage: blend toward accent/deep first (BLEND-scaled, same as
        // dark mode), then pull most of the way back toward white so it
        // stays airy instead of muddy — the pull-back fraction is fixed
        // (it's the "stay light" knob, not the "how strong" knob).
        let tl = color::mix_colors(color::mix_colors(bg, accent, (0.10 * BLEND).min(0.7)), white, 0.45);
        let tr = color::mix_colors(color::mix_colors(bg, accent, (0.05 * BLEND).min(0.7)), white, 0.55);
        let bl = color::mix_colors(color::mix_colors(bg, deep, (0.06 * BLEND).min(0.7)), white, 0.40);
        let br = color::mix_colors(bg, white, (0.11 * BLEND).min(0.7));
        [c32(tl), c32(tr), c32(bl), c32(br)]
    }
}

/// Paint the gradient as one 4-vertex mesh into the background layer, before
/// any panel draws. Cost is a single quad — negligible.
pub fn paint_gradient(ctx: &egui::Context, tk: &Tokens) {
    let rect = ctx.screen_rect();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let [tl, tr, bl, br] = gradient_colors(tk);

    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), tl);
    mesh.colored_vertex(rect.right_top(), tr);
    mesh.colored_vertex(rect.left_bottom(), bl);
    mesh.colored_vertex(rect.right_bottom(), br);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);

    ctx.layer_painter(egui::LayerId::background()).add(egui::Shape::mesh(mesh));
}
