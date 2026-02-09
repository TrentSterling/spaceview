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
- [x] Free space corner bias (sorts last, lands bottom-right)
- [x] Age heatmap mode (color by modified date, red=old, green=recent)

## Up Next
- [ ] File-type coloring (color by extension instead of depth)
- [ ] Duplicate file detection (hash-based)
- [ ] Retake screenshots for new color scheme

## Views and Visualization
- [ ] Extension grouping mode (group all .mp4s, .jpgs together as a treemap)
- [ ] Animated scan visualization (watch treemap build in real-time as files are discovered)
- [ ] Sunburst/radial view (concentric rings, each ring = one depth level)
- [ ] Zoom minimap (small overview rectangle showing viewport position in full treemap)

## Interaction
- [ ] Selection mode (Ctrl+click, shift+click, drag-select multiple files. Show combined size. Batch delete)
- [ ] Comparison mode (scan twice, highlight new/deleted/grown files with colors)
- [ ] Keyboard vim-style navigation (HJKL to move between siblings, Enter to dive in)
- [ ] File type icons/thumbnails (show tiny icons on larger blocks, image thumbnails for photos)

## Performance
- [ ] MFT-based scanning for NTFS (WizTree/Everything approach)
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
