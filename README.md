<p align="center">
  <img src="docs/assets/icon.svg" alt="SpaceView" width="96" />
</p>

<h1 align="center">SpaceView</h1>

<p align="center">
  <strong>See where your disk space goes.</strong><br>
  A fast, visual disk space analyzer inspired by <a href="https://en.wikipedia.org/wiki/SpaceMonger">SpaceMonger</a>.
</p>

<p align="center">
  <img alt="Version" src="https://img.shields.io/badge/version-0.5.2-blue" />
  <img alt="Rust" src="https://img.shields.io/badge/rust-2021-orange" />
  <img alt="egui" src="https://img.shields.io/badge/egui-0.31-green" />
  <img alt="License" src="https://img.shields.io/badge/license-MIT-lightgrey" />
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-0078D6" />
</p>

<p align="center">
  <a href="https://github.com/TrentSterling/SpaceView/releases/latest">Download Latest Release</a>
</p>

---

<p align="center">
  <img src="docs/assets/screenshot.png" alt="SpaceView scanning a drive" width="900" />
</p>

<p align="center">
  <img src="docs/assets/screenshot-about.png" alt="SpaceView About dialog" width="900" />
</p>

---

## Features

- **Treemap Visualization.** Squarified layout shows files and folders as proportionally-sized rectangles. Spot space hogs at a glance.
- **Instant Navigation.** Scroll to zoom, double-click to dive in, right-click to zoom out. Smooth animated transitions.
- **3 Color Themes.** Rainbow, Heatmap, and Pastel. Colors assigned by depth for instant visual hierarchy.
- **Live Scan Progress.** Real-time file count, total size, and scan rate. See elapsed time and files/sec as it runs.
- **Breadcrumb Trail.** Always know where you are in the directory tree. Click any crumb to jump back.
- **Tiny Binary.** ~3.6 MB standalone .exe. No installer, no runtime dependencies. Just download and run.

## Quick Start

### Download

Grab the latest `spaceview.exe` from the [Releases](https://github.com/TrentSterling/SpaceView/releases/latest) page. No installation required. Just run it.

### Build from Source

```bash
git clone https://github.com/TrentSterling/SpaceView.git
cd SpaceView
cargo build --release
```

The binary will be at `target/release/spaceview.exe`.

**Requirements:** [Rust](https://rustup.rs/) (edition 2021)

## Navigation

| Input | Action |
|-------|--------|
| **Scroll** | Zoom in/out at cursor position |
| **Double-click** | Snap zoom into a folder |
| **Right-click** | Zoom out to parent |
| **Drag** | Pan the view |
| **Backspace / Esc** | Zoom out to parent |
| **Breadcrumbs** | Click any breadcrumb to jump there |

## How It Works

SpaceView scans your selected drive or folder, then displays a [squarified treemap](https://www.win.tue.nl/~vanwijk/stm.pdf) where each rectangle's area is proportional to its file/folder size. Larger items are immediately visible. You can spot space hogs at a glance.

The treemap uses **screen-space rendering** like the original SpaceMonger. Child rectangles are laid out in screen pixels with fixed 16px headers, ensuring consistent visual proportions at any zoom level.

### Architecture

```
src/
  main.rs          Entry point, eframe window setup
  app.rs           Main UI: rendering, hit testing, input, themes
  camera.rs        Bounded camera with smooth zoom/pan/snap animations
  scanner.rs       Recursive directory scanner with progress tracking
  world_layout.rs  Lazy LOD layout tree (expand/prune on demand)
  treemap.rs       Squarified treemap algorithm (Bruls et al.)
```

**Key design decisions:**
- Screen-space child layout. No world-space proportional mismatch.
- Two-phase rendering. Headers always drawn on top of children.
- Lazy level-of-detail. Only expand visible directories, prune off-screen ones.
- Bounded camera. Zoom clamped to [1x, 5000x], pan clamped to world bounds.

## Tech Stack

| | |
|---|---|
| Language | Rust (edition 2021) |
| UI Framework | [eframe](https://github.com/emilk/egui)/[egui](https://github.com/emilk/egui) 0.31 |
| File Dialog | [rfd](https://github.com/PolyMeilex/rfd) 0.15 |
| Treemap | Squarified (Bruls, Huizing, van Wijk) |

## Acknowledgments

Inspired by [SpaceMonger](https://en.wikipedia.org/wiki/SpaceMonger) by Sean Werkema. The original treemap disk visualizer for Windows.

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<p align="center">
  Made by <a href="https://github.com/TrentSterling">tront</a>
</p>
