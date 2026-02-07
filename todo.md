# SpaceView Roadmap

## Completed
- [x] Squarified treemap layout (Bruls algorithm)
- [x] Recursive directory scanning with progress/cancellation
- [x] Cached recursive treemap rendering (MAX_DRAW_RECTS=6000)
- [x] Mouse wheel zoom with scroll cooldown
- [x] Click to zoom in, right-click to zoom out
- [x] Breadcrumb navigation
- [x] 8-color palette with depth-based darkening
- [x] Folder headers with adaptive text sizing
- [x] Smooth animated zoom (camera transform, ease-out-cubic, 250ms)

## Up Next
- [ ] File-type coloring (color by extension: video=blue, audio=green, docs=yellow, etc.)
- [ ] Right-click context menu (delete, open in explorer, properties)
- [ ] Search/filter overlay (find files by name/pattern)
- [ ] Keyboard navigation (arrow keys to select, enter to zoom)

## Performance
- [ ] MFT-based scanning for NTFS (WizTree approach — near-instant scan)
- [ ] Radix sort instead of comparison sort (SpaceMonger uses O(n) radix sort)
- [ ] Parallel scanning with jwalk (already in Cargo.toml but not yet used)

## Polish
- [ ] HiDPI support improvements
- [ ] Real-time deletion feedback (update treemap as files are deleted)
- [ ] Export scan results to JSON for offline viewing
- [ ] Dark/light theme toggle
- [ ] Rescan current directory without full restart
