# SpaceView

SpaceMonger-inspired disk space visualizer built with Rust + egui. By tront.

## Tech Stack
- **Language:** Rust (edition 2021)
- **UI Framework:** eframe/egui 0.31
- **Image:** image 0.25 (PNG only)
- **File Dialog:** rfd 0.15
- **System Info:** sysinfo 0.33
- **HTTP:** ureq 2 (sync HTTP client, rustls TLS, for GitHub API version check)
- **Build:** winresource 0.1 (Windows .exe icon embedding)

## Build & Run
```
cargo build          # debug build
cargo build --release # optimized release build
cargo run            # run in debug mode
```

## Architecture (v0.10.0)

### Source Files
- `src/main.rs` - Entry point, creates eframe window (1024x700), loads window icon, `#![windows_subsystem = "windows"]` hides console
- `src/app.rs` - Main UI: SpaceViewApp, continuous camera, screen-space treemap rendering, screen-space hit testing, input handling, themes, welcome/about screens with images, list view, top files view, search/filter, live scan visualization, duplicate detection, extension coloring, cushion shading, rich tooltips
- `build.rs` - Embeds icon.ico into Windows .exe via winresource
- `src/camera.rs` - Continuous Camera with bounds clamping: world_to_screen, screen_to_world, scroll_zoom, drag_pan, snap_to animations. MIN_ZOOM=1.0, MAX_ZOOM=5000
- `src/scanner.rs` - Recursive directory scanner with progress tracking, elapsed time, scan rate, cancellation, and live snapshot channel (scan_directory_live)
- `src/world_layout.rs` - LayoutNode tree in world-space. Lazy expand_visible, prune, ancestor_chain (world_rects used for camera/expand/prune only)
- `src/treemap.rs` - Squarified treemap layout algorithm (Bruls, Huizing, van Wijk)

### Key Design Decisions
- **Screen-space child layout:** Children positioned at render time via `treemap::layout` in screen pixels. Fixed 16px headers, 3px padding, 1.5px border. No proportional world-space mismatch (SpaceMonger-style).
- **Two-phase rendering:** Directories render as body, children, header. Headers drawn ON TOP of children, never obscured.
- **Screen-space hit testing:** Hit test mirrors render traversal. Runs `treemap::layout` at each level to compute exact screen rects.
- **Text clipping:** All text uses `painter.with_clip_rect()` to prevent spilling beyond rect boundaries.
- **Bounded camera:** No nav_stack. Camera with center+zoom, clamped to world bounds. MIN_ZOOM=1.0 (can't zoom past root), MAX_ZOOM=5000 (prevents coordinate overflow). Center clamped so viewport never leaves world_rect.
- **World space (approximate):** Root fills (0,0) to (1.0, aspect_ratio). World_rects used only for camera/expand/prune decisions, not rendering.
- **Lazy LOD:** Directories expand when screen size > 80px, prune when off-screen/tiny. Dynamic expand budget (32 during animation, 8 otherwise).
- **Color themes:** 3 HSL-based themes (Rainbow, Neon, Ocean) using golden angle (137.508 degrees) hue spacing. High lightness (L=0.60-0.65) for vivid SpaceMonger-style colors. Selectable via ComboBox. Colors assigned by depth, never change with zoom.
- **Color pipeline:** Files use base_rgb directly (vivid). Headers at 80% brightness. Bodies at 35% brightness (colored tint, visible as gap borders). Dynamic text_color_for() on headers picks black or white based on luminance. Directory bodies have explicit 1px dark border stroke.
- **Dark/light mode:** Toggle in toolbar. Persisted to prefs.txt. Dark mode default. Only affects UI chrome, treemap stays dark-bodied.
- **Camera-preserving resize:** Window resize remaps camera proportionally instead of resetting to root.
- **Scan progress:** Shows elapsed time and files/sec rate during scans.
- **Welcome screen:** Always shows quickhelp (version, description, shortcuts, Open Folder button). No hide option.
- **About dialog:** Auto-opens on first launch. Escape closes it. "Don't show on startup" checkbox persisted to `%APPDATA%/SpaceView/prefs.txt` (multi-key format). Manual toggle via About button always works.
- **App icon:** `assets/icon.png` (256x256) + `assets/icon.ico` (multi-size). Treemap design matching docs SVG. Window icon via `with_icon()`, .exe icon via `build.rs`.
- **About dialog images:** Icon (64x64) at top, author face (24x24) next to "By tront". Textures lazy-loaded on first About open.
- **Version check:** Background thread on startup hits GitHub releases API via ureq. Polls result in update loop. Shows "Update available" with download link in About dialog. Fails silently on network errors. Uses `is_newer_version()` for semantic comparison.
- **View modes:** Treemap (default), List, Top Files, Types, Duplicates. Tabs in toolbar. ViewMode enum switches central panel rendering.
- **List view:** Sortable directory browser (Name, Size, %, Files columns). Virtual scrolling via show_rows(). Double-click to enter dirs, ".." to go up. Right-click context menu. Breadcrumbs show list_path.
- **Top Files view:** Top 1000 largest files pre-collected on scan thread (no UI freeze). Virtual scrolling. Search filters by name or path.
- **Search bar:** Text filter in toolbar. Filters List and Top Files views by filename/path match.
- **Free space block:** Injected as child node in build_layout. Medium green rgb(60,140,60). Toggle via toolbar button.
- **Right-click context menu:** Available in both Treemap and List views. Open in Explorer, Copy Path, Delete to Recycle Bin.
- **Live scan visualization:** Treemap builds progressively as directories are discovered. `scan_directory_live()` sends partial tree snapshots after each top-level child directory completes. UI drains snapshots each frame, keeping only the newest, and rebuilds the layout. Treemap is interactive (zoom, pan, hover) during scanning.
- **Deferred drops:** When switching drives, old FileNode/WorldLayout trees are moved to a background thread for deallocation. Prevents UI freeze from dropping millions of allocations on the main thread.
- **Scan thread compute:** `compute_time_range()` and file collection run on the scan thread, not the UI thread. Results are bundled with the completion message.
- **Window position persistence:** Window position and size saved to prefs.txt on exit, restored on launch. Supports multi-monitor setups.
- **Extension coloring:** ColorMode::Extension colors files by extension using a map built from cached_extensions (sorted by size). Directories stay depth-colored. Cycles with the color mode button.
- **Duplicate detection:** Background thread after scan completes. Tiered: group by size, partial hash (first 4KB), full hash. Results shown in Duplicates view tab sorted by wasted space.
- **Rich tooltips:** Hover tooltip shows name, size, percentage, file count (dirs), and full path. Uses find_path_for_node lookup.
- **Cushion shading:** 3D edge shadows on file blocks. Light highlight on top/left edges, dark shadow on bottom/right edges. Subtle semi-transparent overlays.

### Navigation
- Scroll: zoom in/out at cursor
- Double-click: snap zoom into folder
- Right-click / Backspace / Esc: zoom out to parent
- Drag: pan view
- Breadcrumbs: built from ancestor_chain() at camera center

### Future / TODO
See `tasks.md` for full backlog (sourced from SpaceMonger, WinDirStat, SpaceSniffer).

**High impact:** Advanced filter/search (SpaceSniffer syntax).

**Medium impact:** Color tagging, filesystem watcher, export/save scans, density slider.

**Nice to have:** CLI, file attributes, drive picker, hardlink detection, NTFS ADS, custom cleanups, portable mode, Linux support, i18n.

### Reference Repos (in SAMPLES/, gitignored)
- SpaceMonger 1.x source. XOR-rect animation, radix sort.
- SpaceSniffer. Real-time update approach.
- WinDirStat. Treemap rendering reference.
