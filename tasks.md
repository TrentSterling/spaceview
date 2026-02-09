# SpaceView Feature Backlog

Features sourced from SpaceMonger (SM), WinDirStat (WDS), and SpaceSniffer (SS).

## High Impact

- [x] **Free space block** (SM, WDS, SS). Show free disk space as a dedicated rectangle in the treemap. Toggle visibility on/off.
- [x] **Right-click context menu** (SM, WDS, SS). Open in Explorer, Properties dialog, Delete to Recycle Bin. Context-sensitive options.
- [x] **Delete files** (SM, WDS, SS). Delete to recycle bin with confirmation prompt. Optional auto-rescan after deletion. Safety lock option to disable delete.
- [x] **File type coloring** (WDS, SS). Color by extension instead of (or in addition to) depth. Extension legend/breakdown panel.
- [x] **Multiple views/tabs** (WDS). File tree list, largest files, duplicates, extension breakdown, treemap. Side-by-side or tabbed.
- [x] **Duplicate file detection** (WDS). Hash-based dedup with tiered hashing strategy (small/medium/large files). Group by hash, show count and total size.
- [ ] **Filter/search** (SS, WDS). Filter by extension, size, age. SpaceSniffer syntax style: `*.jpg;>1mb;<3months`. Real-time view updates.
- [x] **Largest files list** (WDS). Top N files sorted by size. Configurable count. Shows path, size, date.
- [x] **File tree list view** (WDS). Sortable columns: name, size, percentage, date, owner, attributes. Full row selection.

## Medium Impact

- [x] **Cushion shading** (WDS). 3D shading on treemap rects for depth perception. Edge shadows and highlights on file blocks.
- [x] **Rich tooltips** (SM, SS). Hover tooltips with full path, size, percentage, file count for dirs.
- [ ] **Color tagging** (SS). Tag files/folders red/yellow/green/blue via keyboard shortcuts (Ctrl+1-4). Filter by tag color.
- [ ] **Real-time filesystem watcher** (WDS, SS). Detect file changes and auto-update the view. Action logging (create/delete/modify/rename).
- [ ] **Export/save scan results** (WDS, SS). Save to CSV/text/binary snapshot. Reload previous scans. Batch export via command line.
- [ ] **Multiple simultaneous windows** (SS). Multiple views on same scan with different filters and navigation paths.
- [ ] **Extension breakdown panel** (WDS). List of file types with count and total size per extension. Click extension to highlight in treemap.
- [ ] **Density/detail slider** (SM). Control minimum rectangle display size. -3 to +3 range.
- [ ] **Animated zoom** (SM). XOR-rect morphing animation during zoom transitions. 16-frame sequence.
- [x] **Scan pause/resume** (WDS). Suspend and resume scanning. Progress shown in taskbar.

## Nice to Have

- [x] **Drag and drop folders** (SS). Drop a folder onto the window to scan it.
- [ ] **Command line interface** (SS). `spaceview.exe scan C:\ filter *.jpg`. Batch automation support.
- [ ] **File attributes display** (SM, WDS). Show Archive, Hidden, System, Compressed, Encrypted, etc.
- [ ] **Percentage display** (SM, WDS). Show % of parent and % of root alongside size in hover info and file list.
- [ ] **Drive selection dialog** (SM, WDS). Visual drive picker with capacity, free space, drive type icons.
- [ ] **Configurable tooltip delay** (SM). Separate delay settings for name tips and info tips. Per-element toggles.
- [x] **Save/load window state** (SM, WDS). Persist window position and size across sessions.
- [ ] **Hardlink detection** (WDS). Detect and flag hardlinked files. Group under pseudo-folder. Track hardlink count.
- [ ] **NTFS Alternate Data Streams** (SS). Scan and display ADS. Toggle on/off.
- [ ] **Custom cleanup commands** (WDS). User-defined cleanup scripts (up to 10 slots). Configurable per-cleanup: command, recurse, confirm, refresh.
- [ ] **Portable mode** (SS, WDS). Config stored in app directory instead of %APPDATA%. No registry.
- [ ] **Recycle bin cleanup** (WDS). Empty recycle bin from within app.
- [ ] **Localization/i18n** (SM, WDS). Multi-language support. Language selector in settings.
- [ ] **Linux support**. egui works cross-platform. Needs platform-specific paths (preferences, file dialog).
- [ ] **Retake screenshots**. Landing page and GitHub screenshots need updating for v0.6.3 vivid color scheme.
- [ ] **Scan caching / incremental updates**. Cache previous scan, only rescan changed directories.
- [x] **File count in directory headers**. Show number of files/subdirs in header text.
- [ ] **Rollover highlight options** (SM). Configurable hover highlight style and color.
- [ ] **Bias slider** (SM). Control horizontal vs vertical split preference in treemap layout. -20 to +20.
