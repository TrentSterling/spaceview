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
    // Chrome text is chrome, not a document: no I-beam, no text selection on
    // labels (the wordmark was selectable — same fix TrontEQ/TrontSnap carry).
    ctx.style_mut(|s| s.interaction.selectable_labels = false);
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

    // Floating windows (About, dialogs, the gradient editor) carry paragraphs
    // of text and stack OVER already-translucent panels, so they get a
    // near-solid fill — "almost too clear" otherwise (Trent-flagged).
    let window_alpha: u8 = if gradient { 246 } else { 255 };
    v.window_fill =
        Color32::from_rgba_unmultiplied(tk.panel.r(), tk.panel.g(), tk.panel.b(), window_alpha);
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

// ---- background gradient v2 (Discord parity) --------------------------------
//
// The v1 wash mixed every corner toward bg, so the extents never showed real
// color (Trent: "too blended and weak... at the extents we should see the end
// stop colors"). V2 is a true multi-stop ramp, Discord-style:
//   - 2..=4 PEGS derived from the accent via colormagic harmony rules, so the
//     stops can NEVER clash (Discord's trick, our engine).
//   - DIRECTION: any angle, like Discord's Gradient Direction dial.
//   - INTENSITY: 0..1 like Discord's Color Intensity. At 1.0 the background IS
//     the pure peg ramp (panels float on top); at low values it fades to bg.
//   - END-HOLD easing: the ramp saturates to pure first/last peg over the
//     outer ~12% so the extremes read as their color instead of a blend.

#[derive(Clone, Copy, PartialEq)]
pub struct GradientCfg {
    /// Degrees; 0 = left->right, 90 = top->bottom, 135 = TL->BR diagonal.
    pub angle_deg: f32,
    /// 0..1. Discord "Color Intensity". 1.0 = pure peg colors as the ground.
    pub intensity: f32,
    /// 2..=4 color stops (harmony mode).
    pub pegs: u8,
    /// Index into color::HARMONY_RULES used to derive the pegs from the accent.
    pub harmony: u8,
    /// >= 0: index into GRADIENT_PRESETS (curated named ramps, accent ignored).
    /// -1: harmony mode (pegs derived from the live accent).
    /// -2: custom mode (the `custom` pegs below, user-picked, used verbatim).
    pub preset: i16,
    /// Manual pegs for custom mode (first `pegs` entries used).
    pub custom: [Rgb; 4],
}

impl Default for GradientCfg {
    fn default() -> Self {
        GradientCfg {
            angle_deg: 135.0,
            intensity: 0.45,
            pegs: 3,
            harmony: 0,
            preset: -1,
            custom: [[86, 204, 255], [153, 14, 165], [253, 79, 80], [37, 223, 196]],
        }
    }
}

/// Curated, named multi-stop ramps — the "Gradients of the Galaxy / chrome
/// sunset" shelf. Hand-picked hex stops (2-3 per ramp), used verbatim as pegs
/// in dark mode and lifted toward white in light mode. Creative names are
/// load-bearing; nobody rolls "Preset 7".
pub const GRADIENT_PRESETS: &[(&str, &[&str])] = &[
    ("Galaxy Punch", &["#FD4F50", "#990EA5"]),
    ("Nebula Rush", &["#E71B7B", "#8324FB"]),
    ("Ultraviolet", &["#B501AA", "#FD37C8"]),
    ("Solar Flare", &["#FC4D1D", "#F1358A"]),
    ("Chrome Sunset", &["#C0C6CC", "#FFB88C", "#DE4313"]),
    ("Vaporwave", &["#FF6FD8", "#3813C2"]),
    ("Synthwave Drive", &["#DC28B2", "#2A41D2"]),
    ("Deep Space", &["#4D153C", "#B30F40"]),
    ("Golden Hour", &["#FEF528", "#B93B41"]),
    ("Blue Hour", &["#2AA9E9", "#005AFF"]),
    ("Tide Pool", &["#0DBEBA", "#00FFFB"]),
    ("Aurora Sky", &["#00C9FF", "#92FE9D"]),
    ("Toxic Slime", &["#25DFC4", "#E4E518"]),
    ("Matrix Rain", &["#00F032", "#00A0EA"]),
    ("Cherry Cola", &["#EB3349", "#F45C43"]),
    ("Berry Smoothie", &["#FF1B6B", "#45CAFF"]),
    ("Miami Nights", &["#FF0080", "#7928CA", "#4A00E0"]),
    ("Ember Fade", &["#F83600", "#F9D423"]),
    ("Concrete", &["#3A3D42", "#95989E"]),
];

static GRAD_CFG: LazyLock<RwLock<GradientCfg>> = LazyLock::new(|| RwLock::new(GradientCfg::default()));

pub fn gradient_cfg() -> GradientCfg {
    *GRAD_CFG.read().unwrap()
}
pub fn set_gradient_cfg(cfg: GradientCfg) {
    *GRAD_CFG.write().unwrap() = cfg;
}

/// The peg colors: accent -> harmony spread, adapted to the mode so dark
/// themes get deep rich stops and light themes get pastel ones. WCAG isn't a
/// factor here (no text sits on the raw ramp; panels carry the text).
pub fn gradient_pegs(tk: &Tokens) -> Vec<Rgb> {
    let cfg = gradient_cfg();

    // Custom mode: the user's exact colors, no adult supervision.
    if cfg.preset == -2 {
        return cfg.custom[..(cfg.pegs.clamp(2, 4) as usize)].to_vec();
    }

    // Curated preset: designed stops used verbatim (dark), lifted toward
    // white in light mode so the page stays airy under dark text.
    if cfg.preset >= 0 {
        if let Some((_, hexes)) = GRADIENT_PRESETS.get(cfg.preset as usize) {
            return hexes
                .iter()
                .filter_map(|h| color::hex_to_rgb(h))
                .map(|rgb| if tk.dark { rgb } else { color::mix_colors(rgb, [255, 255, 255], 0.40) })
                .collect();
        }
    }

    // Harmony mode: pegs derived from the live accent, clash-proof by rule.
    let rule = color::HARMONY_RULES[(cfg.harmony as usize) % color::HARMONY_RULES.len()];
    let base = color::rgb_to_hsl(rgb_of(tk.accent));
    let spread = color::generate_harmony(base, rule);
    spread
        .into_iter()
        .take((cfg.pegs.clamp(2, 4)) as usize)
        .map(|h| {
            // Mode-adapt lightness: deep + rich on dark, pastel on light.
            let l = if tk.dark { h.l.clamp(20.0, 42.0) } else { h.l.clamp(68.0, 88.0) };
            let s = if tk.dark { h.s.clamp(35.0, 90.0) } else { h.s.clamp(30.0, 75.0) };
            color::hsl_to_rgb(h.h, s, l)
        })
        .collect()
}

/// Sample the peg ramp at t in [0,1], with end-hold easing so the outer ~12%
/// on each side sits at the pure first/last peg.
fn ramp(pegs: &[Rgb], t: f32) -> Rgb {
    let t = ((t - 0.5) * 1.28 + 0.5).clamp(0.0, 1.0);
    let n = pegs.len();
    if n == 1 {
        return pegs[0];
    }
    let scaled = t * (n - 1) as f32;
    let i = (scaled.floor() as usize).min(n - 2);
    let frac = scaled - i as f32;
    color::mix_colors(pegs[i], pegs[i + 1], frac)
}

/// Paint the gradient as a fine vertex-colored grid into the background layer.
/// A grid (not one quad) because the ramp is multi-stop and runs at an
/// arbitrary angle; 16x16 vertices is still a trivially cheap single mesh.
pub fn paint_gradient(ctx: &egui::Context, tk: &Tokens) {
    let rect = ctx.screen_rect();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let cfg = gradient_cfg();
    let pegs = gradient_pegs(tk);
    let bg = rgb_of(tk.bg);

    let a = cfg.angle_deg.to_radians();
    let (dx, dy) = (a.cos(), a.sin());
    let c = rect.center();
    // Projection half-extent of the rect onto the gradient axis.
    let half = (rect.width() * 0.5 * dx.abs()) + (rect.height() * 0.5 * dy.abs());
    let half = half.max(1.0);

    const N: usize = 16;
    let mut mesh = egui::Mesh::default();
    for gy in 0..=N {
        for gx in 0..=N {
            let p = egui::pos2(
                rect.left() + rect.width() * gx as f32 / N as f32,
                rect.top() + rect.height() * gy as f32 / N as f32,
            );
            let t = (((p.x - c.x) * dx + (p.y - c.y) * dy) / half) * 0.5 + 0.5;
            let col = color::mix_colors(bg, ramp(&pegs, t), cfg.intensity.clamp(0.0, 1.0));
            mesh.colored_vertex(p, c32(col));
        }
    }
    let w = (N + 1) as u32;
    for gy in 0..N as u32 {
        for gx in 0..N as u32 {
            let i = gy * w + gx;
            mesh.add_triangle(i, i + 1, i + w);
            mesh.add_triangle(i + 1, i + w + 1, i + w);
        }
    }

    ctx.layer_painter(egui::LayerId::background()).add(egui::Shape::mesh(mesh));
}

/// Sample the final composited ramp (pegs + end-hold easing + intensity mix
/// toward bg) at t in [0,1] — powers the editor's live preview bar.
pub fn ramp_sample(tk: &Tokens, t: f32) -> Color32 {
    let cfg = gradient_cfg();
    let pegs = gradient_pegs(tk);
    c32(color::mix_colors(rgb_of(tk.bg), ramp(&pegs, t), cfg.intensity.clamp(0.0, 1.0)))
}
