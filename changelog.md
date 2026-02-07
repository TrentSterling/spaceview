# Changelog

## v0.2 — Smooth Zoom & Repo Setup
- **Smooth animated zoom:** Camera transform animation with ease-out-cubic easing (250ms). Zooming in/out now smoothly transitions instead of snapping instantly.
- **Project docs:** Added .gitignore, CLAUDE.md, todo.md, changelog.md
- **Git repo:** Initialized repository with clean history
- **Reference samples:** SAMPLES/ directory with SpaceMonger, SpaceSniffer, and WinDirStat sources (gitignored)

## v0.1 — Initial Release
- Squarified treemap layout (Bruls, Huizing, van Wijk algorithm)
- Recursive directory scanning with progress bar and cancellation
- Cached recursive rendering with 6000 rect limit and 5 depth levels
- Mouse wheel zoom in/out with scroll cooldown (0.25s)
- Left-click to zoom into folders, right-click to zoom out
- Breadcrumb navigation bar with clickable path segments
- 8-color palette with depth-based darkening
- Folder headers with name and size labels
- File rectangles with adaptive text sizing
- Drive shortcuts (C-F) for quick scanning
- Status bar showing total size, item count, and hover info
