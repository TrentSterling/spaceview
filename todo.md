# SpaceView Roadmap

## Completed
- [x] Squarified treemap layout (Bruls algorithm)
- [x] Recursive directory scanning with progress/cancellation
- [x] Mouse wheel zoom, click to zoom in, right-click to zoom out
- [x] Breadcrumb navigation
- [x] Smooth animated zoom (camera transform)
- [x] Screen-space rendering pipeline (fixed headers, padding, borders)
- [x] Bounded camera with lazy LOD
- [x] 3 color themes (Rainbow/Neon/Ocean) with golden angle hue spacing
- [x] Dark/light mode toggle (persisted)
- [x] Free space block (toggle on/off)
- [x] Right-click context menu (Open in Explorer, Copy Path, Delete)
- [x] Delete to Recycle Bin with confirmation
- [x] Scan pause/resume
- [x] Drag-and-drop folders
- [x] File count in headers and status bar
- [x] About dialog with version check
- [x] Vivid SpaceMonger-style colors with dynamic header text
- [x] List view with sortable columns and directory navigation
- [x] Top Files view (top 1000 largest files)
- [x] Search/filter bar for List and Top Files
- [x] Virtual scrolling for List and Top Files views

## Up Next
- [ ] File-type coloring (color by extension instead of depth)
- [ ] Duplicate file detection (hash-based)
- [ ] Retake screenshots for new color scheme

## Performance
- [ ] MFT-based scanning for NTFS (WizTree approach)
- [ ] Parallel scanning with jwalk (in Cargo.toml but not yet used)

## Polish
- [ ] Cushion shading (3D depth perception on treemap rects)
- [ ] Rich tooltips (full path, size, date, attributes)
- [ ] Color tagging (tag files red/yellow/green/blue)
- [ ] Filesystem watcher (auto-update on changes)
- [ ] Export/save scan results
- [ ] Extension breakdown panel
- [ ] Drive selection dialog with capacity info
- [ ] CLI support
- [ ] Linux support
