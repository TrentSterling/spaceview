# SpaceView

SpaceMonger-inspired disk space visualizer built with Rust + egui. By tront.

## Tech Stack
- **Language:** Rust (edition 2021)
- **UI Framework:** eframe/egui 0.31
- **Image:** image 0.25 (PNG only)
- **File Dialog:** rfd 0.15
- **System Info:** sysinfo 0.33
- **Build:** winresource 0.1 (Windows .exe icon embedding)

## Build & Run
```
cargo build          # debug build
cargo build --release # optimized release build
cargo run            # run in debug mode
```

## Architecture (v0.5.2)

### Source Files
- `src/main.rs` — Entry point, creates eframe window (1024x700), loads window icon
- `src/app.rs` — Main UI: SpaceViewApp, continuous camera, screen-space treemap rendering, screen-space hit testing, input handling, themes, welcome/about screens with images
- `build.rs` — Embeds icon.ico into Windows .exe via winresource
- `src/camera.rs` — Continuous Camera with bounds clamping: world_to_screen, screen_to_world, scroll_zoom, drag_pan, snap_to animations. MIN_ZOOM=1.0, MAX_ZOOM=5000
- `src/scanner.rs` — Recursive directory scanner with progress tracking, elapsed time, scan rate, and cancellation
- `src/world_layout.rs` — LayoutNode tree in world-space. Lazy expand_visible, prune, ancestor_chain (world_rects used for camera/expand/prune only)
- `src/treemap.rs` — Squarified treemap layout algorithm (Bruls, Huizing, van Wijk)

### Key Design Decisions
- **Screen-space child layout:** Children positioned at render time via `treemap::layout` in screen pixels. Fixed 16px headers, 2px padding, 1px border — no proportional world-space mismatch (SpaceMonger-style)
- **Two-phase rendering:** Directories render as body→children→header (headers drawn ON TOP of children, never obscured)
- **Screen-space hit testing:** Hit test mirrors render traversal — runs `treemap::layout` at each level to compute exact screen rects
- **Text clipping:** All text uses `painter.with_clip_rect()` to prevent spilling beyond rect boundaries
- **Bounded camera:** No nav_stack — Camera with center+zoom, clamped to world bounds. MIN_ZOOM=1.0 (can't zoom past root), MAX_ZOOM=5000 (prevents coordinate overflow). Center clamped so viewport never leaves world_rect
- **World space (approximate):** Root fills (0,0) to (1.0, aspect_ratio). World_rects used only for camera/expand/prune decisions, not rendering
- **Lazy LOD:** Directories expand when screen size > 80px, prune when off-screen/tiny. Dynamic expand budget (32 during animation, 8 otherwise)
- **Color themes:** 3 HSL-based themes (Rainbow, Heatmap, Pastel) — selectable via ComboBox. Colors assigned by depth, never change with zoom
- **Camera-preserving resize:** Window resize remaps camera proportionally instead of resetting to root
- **Scan progress:** Shows elapsed time and files/sec rate during scans
- **Preferences:** `%APPDATA%/SpaceView/prefs.txt` for welcome screen "don't show again"
- **App icon:** `assets/icon.png` (256x256) + `assets/icon.ico` (multi-size) — treemap design matching docs SVG. Window icon via `with_icon()`, .exe icon via `build.rs`
- **About dialog images:** Icon (64x64) at top, author face (24x24) next to "By tront". Textures lazy-loaded on first About open

### Navigation
- Scroll: zoom in/out at cursor
- Double-click: snap zoom into folder
- Right-click / Backspace / Esc: zoom out to parent
- Drag: pan view
- Breadcrumbs: built from ancestor_chain() at camera center

### Reference Repos (in SAMPLES/, gitignored)
- SpaceMonger 1.x source — XOR-rect animation, radix sort
- SpaceSniffer — real-time update approach
- WinDirStat — treemap rendering reference
