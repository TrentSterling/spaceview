# SpaceView

SpaceMonger-inspired disk space visualizer built with Rust + egui.

## Tech Stack
- **Language:** Rust (edition 2021)
- **UI Framework:** eframe/egui 0.31
- **File Dialog:** rfd 0.15
- **System Info:** sysinfo 0.33

## Build & Run
```
cargo build          # debug build
cargo build --release # optimized release build
cargo run            # run in debug mode
```

## Architecture

### Source Files
- `src/main.rs` — Entry point, creates eframe window (1024x700)
- `src/app.rs` — Main UI: SpaceViewApp struct, treemap rendering, navigation, input handling, zoom animation
- `src/scanner.rs` — Recursive directory scanner with progress tracking and cancellation
- `src/treemap.rs` — Squarified treemap layout algorithm (Bruls, Huizing, van Wijk)

### Key Design Decisions
- **Cached layout:** DrawRect cache only recomputes on navigation change or window resize (nav_generation tracking)
- **Max limits:** MAX_DRAW_RECTS=6000, MAX_DEPTH=5, MIN_RECT_PX=3.0 for performance
- **Color system:** 8-color palette with depth-based darkening, folders get headers, files get lighter fills
- **Smooth zoom:** Camera transform animation (scale+offset) with ease-out-cubic easing over 250ms
- **Scroll cooldown:** 0.25s cooldown prevents hyper-zoom on fast scroll wheels

### Navigation Model
- `nav_stack: Vec<usize>` — each entry is a sorted child index at that level
- Zoom in: push index(es) to stack
- Zoom out: pop from stack
- Breadcrumbs built by walking nav_stack through sorted children

### Reference Repos (in SAMPLES/, gitignored)
- SpaceMonger 1.x source — XOR-rect animation, radix sort
- SpaceSniffer — real-time update approach
- WinDirStat — treemap rendering reference
