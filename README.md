<p align="center">
  <img src="docs/assets/icon.svg" alt="SpaceView" width="96" />
</p>

<h1 align="center">SpaceView</h1>

<p align="center">
  <strong>See where your disk space goes.</strong><br>
  A fast, visual disk space analyzer inspired by <a href="https://en.wikipedia.org/wiki/SpaceMonger">SpaceMonger</a>.
</p>

<p align="center">
  <img alt="Version" src="https://img.shields.io/badge/version-0.11.0-blue" />
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
  <img src="docs/assets/screenshot-neon.png" alt="SpaceView treemap with Neon theme" width="900" />
</p>

<p align="center">
  <img src="docs/assets/screenshot-ocean.png" alt="SpaceView full drive scan with Ocean theme" width="900" />
</p>

<p align="center">
  <img src="docs/assets/screenshot-types.png" alt="SpaceView Types view" width="900" />
</p>

---

## Features

- **Treemap Visualization.** Squarified layout shows files and folders as proportionally-sized rectangles. Vivid SpaceMonger-style colors. Cushion shading for 3D depth.
- **Live Scan.** Treemap builds progressively as directories are discovered. Pause, resume, cancel. Drag-and-drop folders.
- **5 View Modes.** Map (treemap), List (sortable directory browser), Top Files (1000 largest), Types (extension treemap), Duplicates. Switch instantly via tabs.
- **Drive Picker.** Visual drive cards with capacity bars on the welcome screen. Click any drive to scan. Toolbar button opens the picker anytime.
- **Extension Breakdown Panel.** Side panel listing every file type by size. Click an extension to highlight matching files in the treemap. Everything else dims.
- **3 Color Modes.** Color by depth, file age (log-scale heatmap), or file extension. 3 themes: Rainbow, Neon, Ocean. Dark/light mode.
- **Duplicate Detection.** Background tiered hashing (size, partial, full). Groups sorted by wasted space. Delete duplicates to reclaim disk.
- **Search/Filter.** Find files by name or path across List, Top Files, Duplicates, and the extension panel.
- **Right-Click Context Menu.** Open in Explorer, Copy Path, Delete to Recycle Bin. Works in all views.
- **Rich Tooltips.** Hover any block for name, size, percentage, file count, and full path.
- **Free Space Block.** See how much disk space is free vs used. Toggle on/off.
- **Tiny Binary.** 3.6 MB standalone .exe. No installer, no runtime dependencies. Just download and run.

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
| **Right-click** | Context menu (or zoom out on empty space) |
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
  app.rs           Main UI: rendering, hit testing, input, themes, drive picker, extension panel
  camera.rs        Bounded camera with smooth zoom/pan/snap animations
  scanner.rs       Recursive directory scanner with progress tracking and live snapshots
  world_layout.rs  Lazy LOD layout tree (expand/prune on demand)
  treemap.rs       Squarified treemap algorithm (Bruls et al.)
```

**Key design decisions:**
- Screen-space child layout. No world-space proportional mismatch.
- Two-phase rendering. Headers always drawn on top of children.
- Lazy level-of-detail. Only expand visible directories, prune off-screen ones.
- Bounded camera. Zoom clamped to [1x, 5000x], pan clamped to world bounds.
- Live scanning. Partial tree snapshots streamed via mpsc channel.
- Deferred drops. Old trees freed on background thread to prevent UI stalls.

## Tech Stack

| | |
|---|---|
| Language | Rust (edition 2021) |
| UI Framework | [eframe](https://github.com/emilk/egui)/[egui](https://github.com/emilk/egui) 0.31 |
| File Dialog | [rfd](https://github.com/PolyMeilex/rfd) 0.15 |
| System Info | [sysinfo](https://github.com/GuillaumeGomez/sysinfo) 0.33 |
| HTTP | [ureq](https://github.com/algesten/ureq) 2 |
| Treemap | Squarified (Bruls, Huizing, van Wijk) |

## Acknowledgments

Inspired by [SpaceMonger](https://en.wikipedia.org/wiki/SpaceMonger) by Sean Werkema. The original treemap disk visualizer for Windows.

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<p align="center">
  Made by <a href="https://github.com/TrentSterling">tront</a> | <a href="https://tront.xyz/SpaceView/">Website</a> | <a href="https://blog.tront.xyz/posts/spaceview/">Blog Post</a>
</p>
