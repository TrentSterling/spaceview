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

## Architecture (v0.12.0)

### Source Files
- `src/main.rs` - Entry point, creates eframe window (1024x700), loads window icon, `#![windows_subsystem = "windows"]` hides console
- `src/app.rs` - Main UI: SpaceViewApp, continuous camera, screen-space treemap rendering, screen-space hit testing, input handling, themes, welcome/about screens with images, list view, top files view, search/filter, live scan visualization, duplicate detection, extension coloring, cushion shading, rich tooltips, extension breakdown panel, drive picker
- `build.rs` - Embeds icon.ico into Windows .exe via winresource
- `src/camera.rs` - Continuous Camera with bounds clamping: world_to_screen, screen_to_world, scroll_zoom, drag_pan, snap_to animations. MIN_ZOOM=1.0, MAX_ZOOM=5000
- `src/scanner.rs` - Recursive directory scanner with progress tracking, elapsed time, scan rate, cancellation, and live snapshot channel (scan_directory_live)
- `src/world_layout.rs` - LayoutNode tree in world-space. Lazy expand_visible (2048-child cap per expansion + "+N more" aggregate tail, 250k global node budget), capacity-releasing prune, cached normalized child layouts (child_norm), ancestor_chain
- `src/treemap.rs` - Squarified treemap layout algorithm (Bruls, Huizing, van Wijk). O(1)-per-item row selection via running min/max; layout_norm for cacheable normalized layouts. Test module pins behavior to the original reference implementation
- `src/stress.rs` - Perf harness: `--synthetic N` in-memory tree generator, `--stress S` scripted camera thrash, per-second CSV metrics (frame ms, layout calls, shapes, node count, RSS)

### Key Design Decisions
- **Cached normalized layouts (v0.12):** Each directory's squarified child layout is computed ONCE at expansion, normalized to a `1.0 x aspect` box, and stored on the LayoutNode (`child_norm` + `child_norm_aspect`). Render, hit test, and minimap scale the cached rects into the screen content rect every frame; `treemap::layout` never runs in the per-frame path. World rects derive from the same normalized layout, so world-space decisions and rendering always agree. Fixed 16px headers, 3px padding, 1.5px border.
- **Batched block mesh (v0.12):** All opaque rects (dir bodies, borders, headers, file blocks) accumulate into ONE vertex-colored `egui::Mesh` in traversal order (painter's algorithm holds within a mesh); text goes through the painter and always draws on top. Cushion shading is a per-vertex gradient (light top-left, dark bottom-right corners), not overlay rects. The mesh z-slot is reserved via `painter.add(Shape::Noop)` + `painter.set` before traversal.
- **Bounded memory (v0.12):** `EXPAND_CHILD_CAP = 2048` per expansion with a "+N more" aggregate tail node (`is_aggregate`, `child_index = AGGREGATE_INDEX`); `NODE_BUDGET = 250_000` global cap gates expansion; prune assigns fresh Vecs (never `clear()`, which retains capacity) and runs every 15 frames, or every frame while over budget. `WorldLayout.live_nodes` tracks the count incrementally; the stress harness cross-checks it against a full walk.
- **Two-phase rendering:** Directories render as body, children, header. Headers drawn ON TOP of children, never obscured.
- **Screen-space hit testing:** Hit test mirrors render traversal over the cached layouts and accumulates the `child_index` trail; `HoveredInfo.path_indices` resolves the real FileNode path in O(depth) via `resolve_path` (no name+size searching; aggregates resolve to None and get no filesystem actions).
- **Text clipping:** All text uses `painter.with_clip_rect()` to prevent spilling beyond rect boundaries.
- **Bounded camera:** No nav_stack. Camera with center+zoom, clamped to world bounds. MIN_ZOOM=1.0 (can't zoom past root), MAX_ZOOM=5000 (prevents coordinate overflow). Center clamped so viewport never leaves world_rect.
- **World space (approximate):** Root fills (0,0) to (1.0, aspect_ratio). World_rects used only for camera/expand/prune decisions, not rendering.
- **Lazy LOD:** Directories expand when screen size > 80px, prune when off-screen/tiny. Dynamic expand budget (32 during animation, 8 otherwise).
- **Color themes:** 3 HSL-based themes (Rainbow, Neon, Ocean) using golden angle (137.508 degrees) hue spacing. High lightness (L=0.60-0.65) for vivid SpaceMonger-style colors. Selectable via ComboBox. Colors assigned by depth, never change with zoom.
- **Color pipeline:** Files use base_rgb directly (vivid). Headers at 80% brightness. Bodies at 35% brightness (colored tint, visible as gap borders). Dynamic text_color_for() on headers picks black or white based on luminance. Directory bodies have explicit 1px dark border stroke.
- **Dark/light mode:** Toggle in toolbar. Persisted to prefs.txt. Dark mode default. Only affects UI chrome, treemap stays dark-bodied.
- **Camera-preserving resize:** Window resize remaps camera proportionally instead of resetting to root.
- **Scan progress:** Shows elapsed time and files/sec rate during scans.
- **Welcome screen:** Shows drive cards with capacity bars (blue/yellow/red by usage), name, type, filesystem. Click a drive to scan. "Open Folder..." button below as fallback. Keyboard shortcuts at the bottom.
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
- **Cushion shading:** 3D shading on file blocks via per-vertex colors in the batched mesh: lightened top-left corner, darkened bottom-right, diagonal gradient from bilinear interpolation. Zero extra geometry.
- **Drive picker:** DriveInfo struct + enumerate_drives() using sysinfo::Disks. Visual drive cards with capacity bars on welcome screen. Toolbar "Drives" button opens picker dialog (egui::Window). Replaces hardcoded C/D/E/F buttons.
- **Extension breakdown panel:** SidePanel::right with virtual-scrolled extension list. Colored swatches, selectable labels (extension + size + count), thin percentage bars. Click to filter treemap (dims non-matching files via gamma_multiply(0.25)). Click same extension to clear. Search filters the list. Auto-switches to ColorMode::Extension when filtering. Resizable (180-350px, default 220).
- **Extension filter dimming:** render_node() accepts selected_ext parameter. Non-matching file blocks dimmed to 25% brightness. Directory headers/bodies not dimmed. Free space dimmed when filter active.

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

**Nice to have:** CLI, file attributes, hardlink detection, NTFS ADS, custom cleanups, portable mode, Linux support, i18n.

### Reference Repos (in SAMPLES/, gitignored)
- SpaceMonger 1.x source. XOR-rect animation, radix sort.
- SpaceSniffer. Real-time update approach.
- WinDirStat. Treemap rendering reference.
