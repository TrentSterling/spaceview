# Changelog

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
