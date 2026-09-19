//! Scripted screenshot mode for the landing page and store listings.
//!
//! `spaceview.exe --synthetic 20000 --shots DIR` renders a fixed sequence of
//! theme presets, view modes and color modes over the in-memory synthetic tree
//! (no real disk is ever shown), captures each frame through eframe's
//! viewport screenshot command, writes `DIR/<file>.png` and exits. No input
//! is needed; the app drives itself. Zero overhead when the flag is absent.

use std::path::PathBuf;

/// One frame to capture. `view`: 0 treemap, 1 list, 2 largest files,
/// 3 extensions, 4 duplicates. `color`: 0 depth, 1 age, 2 extension.
pub struct Shot {
    pub file: &'static str,
    pub preset: &'static str,
    pub dark: bool,
    pub view: u8,
    pub color: u8,
    /// Treemap block palette: 0 rainbow, 1 neon, 2 ocean.
    pub treemap: u8,
}

pub const SHOTS: &[Shot] = &[
    Shot { file: "hero-chrome-sunset.png", preset: "Chrome Sunset", dark: true, view: 0, color: 0, treemap: 0 },
    Shot { file: "treemap-age-deep-space.png", preset: "Deep Space", dark: true, view: 0, color: 1, treemap: 1 },
    Shot { file: "treemap-types-aurora-sky.png", preset: "Aurora Sky", dark: true, view: 0, color: 2, treemap: 2 },
    Shot { file: "treemap-light-golden-hour.png", preset: "Golden Hour", dark: false, view: 0, color: 0, treemap: 0 },
    Shot { file: "largest-files-vaporwave.png", preset: "Vaporwave", dark: true, view: 2, color: 0, treemap: 1 },
    Shot { file: "extensions-tide-pool.png", preset: "Tide Pool", dark: true, view: 3, color: 2, treemap: 2 },
    Shot { file: "list-concrete-light.png", preset: "Concrete", dark: false, view: 1, color: 0, treemap: 0 },
    Shot { file: "treemap-matrix-rain.png", preset: "Matrix Rain", dark: true, view: 0, color: 0, treemap: 1 },
];

/// Frames to let layout, LOD expansion and the camera settle before capturing.
pub const SETTLE_FRAMES: u32 = 45;

pub struct ShotScript {
    pub dir: PathBuf,
    pub idx: usize,
    pub frames: u32,
    pub pending: bool,
}

impl ShotScript {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir, idx: 0, frames: 0, pending: false }
    }
}

/// `--shots DIR` from argv.
pub fn parse_flag_path(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

/// Write an egui ColorImage as PNG.
pub fn save_png(path: &std::path::Path, img: &eframe::egui::ColorImage) -> Result<(), String> {
    let (w, h) = (img.width() as u32, img.height() as u32);
    let buf = image::RgbaImage::from_raw(w, h, img.as_raw().to_vec())
        .ok_or_else(|| "pixel buffer size mismatch".to_string())?;
    buf.save(path).map_err(|e| e.to_string())
}
