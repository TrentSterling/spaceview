# Changelog

## v0.12.0 - Renderer Overhaul: Cached Layouts, Bounded Memory, Crashproofing
- **Cached treemap layouts.** Each directory's squarified layout is computed once at expansion (normalized, camera-independent) and reused every frame by render, hit test, and minimap. Previously all three re-ran the full layout algorithm per visible directory per frame (measured at 100-316 layout calls/frame during zoom thrash; now 0-6).
- **Batched block mesh.** All treemap rects (bodies, borders, headers, file blocks) draw as a single vertex-colored mesh instead of thousands of individually tessellated rounded rects. Cushion shading is now a per-vertex gradient (free) instead of 4 overlay rects per block. Huge win on weak/software-GL machines.
- **Bounded memory.** Directory expansion caps at 2048 children (rest collapse into a "+N more" aggregate cell), a 250k global node budget gates expansion, and prune actually releases heap capacity (the old `Vec::clear()` retained it forever, ratcheting RSS up ~90MB/min under scroll thrash until eventual out-of-memory).
- **Faster squarify.** Row selection is O(1) per item via running min/max (was accidentally quadratic per row); verified byte-identical layouts against the old implementation.
- **Correct hover paths.** Hovered/right-clicked nodes resolve their real path through the layout tree's index chain in O(depth). The old name+size whole-tree search ran per hover frame AND could resolve to the wrong file on collisions; Delete to Recycle Bin now always targets the exact block you clicked.
- **Camera smoothing fixed.** Zoom/pan easing is now frame-rate independent (the old math inverted it: slow machines got slower, longer animation tails).
- **Crash diagnostics.** Panics write to `%APPDATA%/SpaceView/panic.log` with a backtrace (the windowed exe has no console, so crashes used to vanish without a trace).
- **Perf harness.** `--synthetic N` generates an in-memory N-file tree; `--stress S` runs a scripted camera thrash for S seconds, logging frame times, layout calls, node counts, and RSS to `stress_log.csv`. 3M-node/60s acceptance run: vsync-locked 16.7ms frames, flat RSS, zero layout recomputes at steady state.
- **Live-scan snapshots throttled** to 2/sec (each snapshot deep-clones the partial tree; on huge drives that cost grew quadratically).
- **Font sizes quantized** to whole pixels so zooming can't mint unbounded glyph-atlas entries.
- **Release profile** switched from size-optimized (`opt-level="z"`) to speed-optimized (`opt-level=3`).

## v0.11.0 - Extension Breakdown Panel, Drive Picker
- **Extension breakdown panel.** Side panel listing every file type by size, count, and percentage. Colored swatches match treemap colors. Click an extension to filter the treemap (dims non-matching files to 25% brightness). Click again to clear. Search filters the list. Resizable panel (180-350px).
- **Drive picker.** Visual drive cards with capacity bars on the welcome screen. Shows drive name, filesystem, type, free/total space. Blue (<75%), yellow (75-90%), red (>90%) capacity bars. Click to scan. Toolbar "Drives" button opens picker dialog anytime.
- **Extension filter dimming.** Treemap dims non-matching file blocks via gamma_multiply(0.25). Directory headers and bodies not dimmed. Free space dimmed. Auto-switches to extension color mode when filtering.
- **Removed hardcoded drive buttons.** Replaced C/D/E/F buttons with the visual drive picker.

## v0.10.0 - Live Scan, Extension Coloring, Duplicates, Cushion Shading
- **Live scan visualization.** Treemap builds progressively during scanning. Interactive (zoom, pan, hover) while scanning.
- **Extension coloring.** ColorMode::Extension colors files by extension. Cycles via toolbar button.
- **Duplicate detection.** Background tiered hashing (size, partial 4KB, full). Duplicates view tab sorted by wasted space.
- **Cushion shading.** 3D edge shadows on file blocks. Light top/left, dark bottom/right.
- **Rich tooltips.** Name, size, percentage, file count, full path on hover.
- **Deferred drops.** Old trees freed on background thread when switching drives.
- **Window position persistence.** Saved to prefs.txt on exit, restored on launch.

## v0.9.0 - Extension View, Minimap
- **Extension grouping view.** Types tab shows treemap of file extensions by total size.
- **Zoom minimap.** Bottom-right corner shows overview with viewport indicator when zoomed in.

## v0.8.0 - Age Heatmap, Free Space Corner Bias
- **Age heatmap mode.** New "Age Map" color mode. Files colored by last modified date on a red-yellow-green gradient. Red = old untouched files, green = recently modified. Directories inherit the newest child's timestamp. Toggle via toolbar button. Status bar shows color legend.
- **Free space corner bias.** Free space block now always sorts to the end, so the treemap places it in the bottom-right corner instead of jumping around based on relative size.
- **Modified timestamps.** Scanner now captures file modified times. Stored in FileNode and propagated through LayoutNode for rendering.

## v0.7.0 - Views, Search, Virtual Scrolling
- **View mode tabs.** Map (treemap), List (sortable directory browser), Top Files (1000 largest files). Switch instantly via toolbar tabs.
- **List view.** Sortable columns: Name, Size, %, Files. Double-click directories to navigate. ".." to go up. Right-click context menu with Open in Explorer, Copy Path, Delete.
- **Top Files view.** Top 1000 largest files collected on the scan thread. Zero UI freeze when switching tabs. Virtual scrolling for smooth performance.
- **Search bar.** Text filter in toolbar. Filters List and Top Files views by filename or path. Case-insensitive.
- **Virtual scrolling.** Both List and Top Files use ScrollArea::show_rows(). Only visible rows render each frame.
- **Free space fix.** Removed canonicalize() that added \\?\ prefix on Windows, breaking path matching. Free space block now works correctly.
- **Free space size fix.** Fixed root.size inflation when free space node was re-added during layout rebuilds.

## v0.6.3 - Vivid Colors, Readable Headers
- **Vivid SpaceMonger-style colors.** HSL lightness bumped to 0.60-0.65 across all themes. Files use base_rgb directly.
- **Dynamic header text.** text_color_for() picks black or white based on luminance. No more hardcoded white text.
- **Visible borders.** 1px dark stroke on directory bodies. Body color at 35% brightness shows colored gap borders.
- **Bright free space.** Green block changed from rgb(30,60,30) to rgb(60,140,60).

## v0.6.2 - Delete, Pause/Resume
- **Delete to Recycle Bin.** Right-click context menu option. Confirmation dialog. Auto-rescans after delete.
- **Scan pause/resume.** Pause and resume button during scanning.

## v0.6.1 - Free Space, Context Menu, Drag-Drop
- **Free space block.** Shows free disk space as a dedicated rectangle. Toggle via toolbar button.
- **Right-click context menu.** Zoom In/Out, Open in Explorer, Copy Path, Delete to Recycle Bin.
- **Drag-and-drop.** Drop folders onto the window to scan them.
- **File counts.** Shown in directory headers and status bar.

## v0.6.0 - Golden Angle Themes, Dark Mode
- **3 color themes.** Rainbow, Neon, Ocean. Golden angle hue spacing for max contrast.
- **Dark/light mode.** Toggle in toolbar. Persisted to prefs.txt.
- **Camera-preserving resize.** Window resize remaps camera proportionally.
- **About dialog.** Auto-opens on first launch. Version check via GitHub API.

## v0.5.x - Screen-Space Rendering
- **Screen-space child layout.** Children positioned at render time in screen pixels. Fixed 16px headers, 3px padding.
- **Two-phase rendering.** Headers drawn on top of children.
- **Screen-space hit testing.** Mirrors render traversal exactly.
- **Bounded camera.** MIN_ZOOM=1.0, MAX_ZOOM=5000. Center clamped to world bounds.
- **Lazy LOD.** Expand visible directories, prune off-screen ones.

## v0.2 - Smooth Zoom
- **Smooth animated zoom.** Camera transform animation with ease-out-cubic easing.
- **Project docs.** Added .gitignore, CLAUDE.md, todo.md, changelog.md.

## v0.1 - Initial Release
- Squarified treemap layout (Bruls, Huizing, van Wijk algorithm).
- Recursive directory scanning with progress bar and cancellation.
- Mouse wheel zoom, click to zoom in, right-click to zoom out.
- Breadcrumb navigation. 8-color palette. Drive shortcuts (C-F).
