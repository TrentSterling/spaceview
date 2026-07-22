//! Stress-test instrumentation for reproducing the crash-under-thrash and
//! weak-hardware-slowness symptoms BEFORE any fix exists. Entirely additive:
//! zero overhead when neither --synthetic nor --stress is passed on the CLI.

use crate::camera::Camera;
use crate::scanner::FileNode;
use eframe::egui;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

// Output lands in the process cwd (run from the repo root during dev).
// Gitignored.
const METRICS_CSV_PATH: &str = "stress_log.csv";
const SUMMARY_PATH: &str = "stress_summary.txt";

/// Count of treemap::layout() invocations, reset every logging tick.
pub static LAYOUT_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Count of painter shape emissions (one per node actually drawn), reset every logging tick.
pub static SHAPE_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn inc_layout_calls() {
    LAYOUT_CALLS.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_shapes() {
    SHAPE_COUNT.fetch_add(1, Ordering::Relaxed);
}

// Panics are captured by main.rs's install_panic_log (%APPDATA%/SpaceView/panic.log).

pub fn parse_flag_usize(args: &[String], flag: &str) -> Option<usize> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

pub fn parse_flag_f32(args: &[String], flag: &str) -> Option<f32> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

// ===================== Deterministic xorshift64 PRNG =====================
// Avoids pulling in the `rand` crate for a diagnostic-only tool.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    pub fn range_u64(&mut self, lo: u64, hi: u64) -> u64 {
        let span = hi.saturating_sub(lo).max(1);
        lo + self.next_u64() % span
    }
}

fn synthetic_file_size(rng: &mut Rng) -> u64 {
    let r = rng.next_f64();
    if r < 0.05 {
        0 // zero-byte files: common in real scans (placeholders, locks, empty configs)
    } else if r < 0.85 {
        rng.range_u64(100, 50_000)
    } else if r < 0.98 {
        rng.range_u64(50_000, 5_000_000)
    } else {
        rng.range_u64(5_000_000, 500_000_000)
    }
}

/// Generate an in-memory synthetic FileNode tree with ~total_files total files,
/// mixing: wide "node_modules"-style directories (thousands of direct children,
/// the shape that trips squarify's O(n) recursion depth / O(n^2) row-building),
/// deep chains (15-25 levels), and a moderate-branching filler subtree.
pub fn generate_synthetic_tree(total_files: usize) -> FileNode {
    let mut rng = Rng::new(0xC0FFEE_u64 ^ total_files as u64);

    let mut root = FileNode {
        name: "SyntheticRoot".to_string(),
        path: PathBuf::from("SYN:\\root"),
        size: 0,
        is_dir: true,
        file_count: 0,
        modified: 0,
        children: Vec::new(),
    };

    let mut remaining = total_files;

    // 1. Wide flat directories (~40% of budget): the node_modules / browser-cache shape.
    let wide_budget = (total_files as f64 * 0.4) as usize;
    let mut wide_used = 0usize;
    let mut wide_idx = 0;
    while wide_used < wide_budget && remaining > 0 && wide_idx < 60 {
        let n_children = (rng.range_u64(3_000, 15_000) as usize).min(remaining);
        if n_children == 0 {
            break;
        }
        let dir = make_wide_dir(&mut rng, format!("node_modules_{}", wide_idx), n_children);
        wide_used += n_children;
        remaining = remaining.saturating_sub(n_children);
        root.children.push(dir);
        wide_idx += 1;
    }

    // 2. Deep chains (~10% of budget): 15-25 levels, stresses recursion depth
    // in code paths that walk ancestor/descendant chains.
    let deep_budget = (total_files as f64 * 0.1) as usize;
    let mut deep_used = 0usize;
    let mut deep_idx = 0;
    while deep_used < deep_budget && remaining > 0 && deep_idx < 40 {
        let depth = rng.range_u64(15, 26) as usize;
        let name = format!("deep_chain_{}", deep_idx);
        let (chain, used) = make_deep_chain(&mut rng, name, depth, remaining);
        deep_used += used;
        remaining = remaining.saturating_sub(used);
        root.children.push(chain);
        deep_idx += 1;
        if used == 0 {
            break;
        }
    }

    // 3. Remaining budget: moderate-branching filler tree.
    if remaining > 0 {
        let filler = make_branching_tree(&mut rng, "data".to_string(), remaining, 6);
        root.children.push(filler);
    }

    finalize_tree(&mut root);
    root
}

fn make_wide_dir(rng: &mut Rng, name: String, n: usize) -> FileNode {
    let path = PathBuf::from(format!("SYN:\\{}", name));
    let mut dir = FileNode {
        name,
        path: path.clone(),
        size: 0,
        is_dir: true,
        file_count: 0,
        modified: 0,
        children: Vec::with_capacity(n),
    };
    for i in 0..n {
        let size = synthetic_file_size(rng);
        dir.children.push(FileNode {
            name: format!("file_{}.js", i),
            path: path.join(format!("file_{}.js", i)),
            size,
            is_dir: false,
            file_count: 0,
            modified: 1_700_000_000 + (i as u64 % 100_000),
            children: Vec::new(),
        });
    }
    dir
}

fn make_deep_chain(rng: &mut Rng, name: String, depth: usize, budget: usize) -> (FileNode, usize) {
    fn build(
        rng: &mut Rng,
        path: PathBuf,
        name: String,
        depth: usize,
        budget: usize,
        used: &mut usize,
    ) -> FileNode {
        let mut dir = FileNode {
            name,
            path: path.clone(),
            size: 0,
            is_dir: true,
            file_count: 0,
            modified: 0,
            children: Vec::new(),
        };
        let n_files = rng.range_u64(1, 4) as usize;
        for i in 0..n_files {
            if *used >= budget {
                break;
            }
            let size = synthetic_file_size(rng);
            *used += 1;
            dir.children.push(FileNode {
                name: format!("f{}.dat", i),
                path: path.join(format!("f{}.dat", i)),
                size,
                is_dir: false,
                file_count: 0,
                modified: 1_700_000_000,
                children: Vec::new(),
            });
        }
        if depth > 0 && *used < budget {
            let child_name = format!("level_{}", depth);
            let child_path = path.join(&child_name);
            let child = build(rng, child_path, child_name, depth - 1, budget, used);
            dir.children.push(child);
        }
        dir
    }

    let mut used = 0usize;
    let root_path = PathBuf::from(format!("SYN:\\{}", name));
    let node = build(rng, root_path, name, depth, budget, &mut used);
    (node, used)
}

fn make_branching_tree(rng: &mut Rng, name: String, budget: usize, max_depth: usize) -> FileNode {
    fn build(
        rng: &mut Rng,
        path: PathBuf,
        name: String,
        remaining: &mut usize,
        depth: usize,
        max_depth: usize,
    ) -> FileNode {
        let mut dir = FileNode {
            name,
            path: path.clone(),
            size: 0,
            is_dir: true,
            file_count: 0,
            modified: 0,
            children: Vec::new(),
        };
        if *remaining == 0 {
            return dir;
        }
        if depth >= max_depth || *remaining < 20 {
            let n = (*remaining).min(2000);
            for i in 0..n {
                let size = synthetic_file_size(rng);
                *remaining -= 1;
                dir.children.push(FileNode {
                    name: format!("leaf_{}.bin", i),
                    path: path.join(format!("leaf_{}.bin", i)),
                    size,
                    is_dir: false,
                    file_count: 0,
                    modified: 1_700_000_000,
                    children: Vec::new(),
                });
            }
            return dir;
        }
        let n_subdirs = rng.range_u64(3, 12) as usize;
        for s in 0..n_subdirs {
            if *remaining == 0 {
                break;
            }
            let sub_name = format!("sub_{}", s);
            let sub_path = path.join(&sub_name);
            let child = build(rng, sub_path, sub_name, remaining, depth + 1, max_depth);
            if child.file_count > 0 || !child.children.is_empty() {
                dir.children.push(child);
            }
        }
        dir
    }

    let mut remaining = budget;
    let path = PathBuf::from(format!("SYN:\\{}", name));
    build(rng, path, name, &mut remaining, 0, max_depth)
}

/// Bottom-up recompute of size/file_count/modified/sort, mirroring what
/// scanner::scan_directory does for a real scan.
fn finalize_tree(node: &mut FileNode) {
    if node.is_dir {
        let mut size = 0u64;
        let mut file_count = 0u64;
        for c in &mut node.children {
            finalize_tree(c);
            size += c.size;
            file_count += if c.is_dir { c.file_count } else { 1 };
        }
        node.size = size;
        node.file_count = file_count;
        node.children.sort_by(|a, b| b.size.cmp(&a.size));
        node.modified = node.children.iter().map(|c| c.modified).max().unwrap_or(0);
    }
}

// ===================== Automated camera-thrash script =====================

enum Phase {
    ZoomIn,
    Pan,
    ZoomOut,
}

const ZOOM_IN_DURATION: f32 = 1.0;
const PAN_DURATION: f32 = 0.6;
const ZOOM_OUT_DURATION: f32 = 1.0;
const ZOOM_RATE_PER_SEC: f32 = 40.0; // scroll "notches" per second, matches scroll_zoom's own scale

pub struct StressScript {
    duration: f32,
    start: Instant,
    rng: Rng,
    phase: Phase,
    phase_elapsed: f32,
    target_focus: egui::Pos2,
    pan_delta: egui::Vec2,
    pub cycle: u32,
}

impl StressScript {
    pub fn new(duration: f32) -> Self {
        let mut rng = Rng::new(0xFEED_FACE);
        let target_focus = random_focus(&mut rng);
        StressScript {
            duration,
            start: Instant::now(),
            rng,
            phase: Phase::ZoomIn,
            phase_elapsed: 0.0,
            target_focus,
            pan_delta: egui::vec2(0.0, 0.0),
            cycle: 0,
        }
    }

    pub fn elapsed(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn finished(&self) -> bool {
        self.elapsed() >= self.duration
    }

    /// Advance the scripted thrash by one frame, driving the camera through
    /// its real scroll_zoom / drag_pan entry points (no OS input injection).
    pub fn step(&mut self, camera: &mut Camera, viewport: egui::Rect, dt: f32) {
        self.phase_elapsed += dt;
        match self.phase {
            Phase::ZoomIn => {
                camera.scroll_zoom(ZOOM_RATE_PER_SEC * dt, self.target_focus, viewport);
                if self.phase_elapsed >= ZOOM_IN_DURATION {
                    self.phase = Phase::Pan;
                    self.phase_elapsed = 0.0;
                    self.pan_delta = egui::vec2(
                        (self.rng.next_f64() as f32 - 0.5) * 0.02,
                        (self.rng.next_f64() as f32 - 0.5) * 0.02,
                    );
                }
            }
            Phase::Pan => {
                camera.drag_pan(self.pan_delta, viewport);
                if self.phase_elapsed >= PAN_DURATION {
                    self.phase = Phase::ZoomOut;
                    self.phase_elapsed = 0.0;
                }
            }
            Phase::ZoomOut => {
                let focus = camera.center;
                camera.scroll_zoom(-ZOOM_RATE_PER_SEC * dt, focus, viewport);
                if self.phase_elapsed >= ZOOM_OUT_DURATION || camera.zoom <= 1.05 {
                    self.phase = Phase::ZoomIn;
                    self.phase_elapsed = 0.0;
                    self.cycle += 1;
                    self.target_focus = random_focus(&mut self.rng);
                }
            }
        }
    }
}

fn random_focus(rng: &mut Rng) -> egui::Pos2 {
    let x = 0.02 + rng.next_f64() as f32 * 0.96;
    let y = 0.02 + rng.next_f64() as f32 * 0.96;
    egui::pos2(x, y)
}

// ===================== Metrics logging =====================

pub struct MetricsLogger {
    file: std::fs::File,
    start: Instant,
    last_log: Instant,
    frame_count: u32,
    frame_sum_ms: f64,
    frame_max_ms: f64,
    sys: sysinfo::System,
    pid: sysinfo::Pid,

    // Whole-run accumulators for the final summary.
    total_frames: u64,
    total_frame_sum_ms: f64,
    global_max_ms: f64,
    peak_layout_nodes: usize,
    peak_rss_mb: f64,
    last_layout_nodes: usize,
    last_rss_mb: f64,
    rows_written: u32,
}

impl MetricsLogger {
    pub fn new() -> Self {
        let mut file = std::fs::File::create(METRICS_CSV_PATH).expect("create stress_log.csv");
        writeln!(
            file,
            "elapsed_s,frame_ms_avg,frame_ms_max,layout_calls_per_frame,shapes_per_frame,layout_node_count,rss_mb"
        )
        .ok();
        let pid = sysinfo::get_current_pid().expect("get_current_pid");
        let sys = sysinfo::System::new();
        MetricsLogger {
            file,
            start: Instant::now(),
            last_log: Instant::now(),
            frame_count: 0,
            frame_sum_ms: 0.0,
            frame_max_ms: 0.0,
            sys,
            pid,
            total_frames: 0,
            total_frame_sum_ms: 0.0,
            global_max_ms: 0.0,
            peak_layout_nodes: 0,
            peak_rss_mb: 0.0,
            last_layout_nodes: 0,
            last_rss_mb: 0.0,
            rows_written: 0,
        }
    }

    /// Record one frame's cost. Returns true if a full second has elapsed and
    /// the caller should provide a fresh layout-node-count snapshot via flush_log.
    pub fn tick(&mut self, frame_ms: f64) -> bool {
        self.frame_count += 1;
        self.frame_sum_ms += frame_ms;
        if frame_ms > self.frame_max_ms {
            self.frame_max_ms = frame_ms;
        }
        self.total_frames += 1;
        self.total_frame_sum_ms += frame_ms;
        if frame_ms > self.global_max_ms {
            self.global_max_ms = frame_ms;
        }
        self.last_log.elapsed().as_secs_f32() >= 1.0
    }

    pub fn flush_log(&mut self, layout_node_count: usize) {
        let elapsed_s = self.start.elapsed().as_secs_f64();
        let avg = if self.frame_count > 0 {
            self.frame_sum_ms / self.frame_count as f64
        } else {
            0.0
        };
        let layout_calls = LAYOUT_CALLS.swap(0, Ordering::Relaxed);
        let shapes = SHAPE_COUNT.swap(0, Ordering::Relaxed);
        let denom = self.frame_count.max(1) as f64;
        let layout_calls_per_frame = layout_calls as f64 / denom;
        let shapes_per_frame = shapes as f64 / denom;

        self.sys
            .refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.pid]), true);
        let rss_mb = self
            .sys
            .process(self.pid)
            .map(|p| p.memory() as f64 / 1_048_576.0)
            .unwrap_or(0.0);

        if layout_node_count > self.peak_layout_nodes {
            self.peak_layout_nodes = layout_node_count;
        }
        if rss_mb > self.peak_rss_mb {
            self.peak_rss_mb = rss_mb;
        }
        self.last_layout_nodes = layout_node_count;
        self.last_rss_mb = rss_mb;
        self.rows_written += 1;

        let line = format!(
            "{:.2},{:.3},{:.3},{:.1},{:.1},{},{:.1}\n",
            elapsed_s, avg, self.frame_max_ms, layout_calls_per_frame, shapes_per_frame, layout_node_count, rss_mb
        );
        let _ = self.file.write_all(line.as_bytes());
        let _ = self.file.flush();

        eprintln!(
            "[stress] t={:.1}s frame_avg={:.2}ms frame_max={:.2}ms layout_calls/f={:.1} shapes/f={:.1} nodes={} rss={:.1}MB",
            elapsed_s, avg, self.frame_max_ms, layout_calls_per_frame, shapes_per_frame, layout_node_count, rss_mb
        );

        self.frame_count = 0;
        self.frame_sum_ms = 0.0;
        self.frame_max_ms = 0.0;
        self.last_log = Instant::now();
    }

    pub fn write_summary(&self, cycles: u32) {
        let avg_all = if self.total_frames > 0 {
            self.total_frame_sum_ms / self.total_frames as f64
        } else {
            0.0
        };
        let summary = format!(
            "SpaceView stress run summary\n\
             total_frames={}\n\
             frame_ms_avg_overall={:.3}\n\
             frame_ms_max_overall={:.3}\n\
             peak_layout_node_count={}\n\
             final_layout_node_count={}\n\
             peak_rss_mb={:.1}\n\
             final_rss_mb={:.1}\n\
             csv_rows_written={}\n\
             thrash_cycles={}\n",
            self.total_frames,
            avg_all,
            self.global_max_ms,
            self.peak_layout_nodes,
            self.last_layout_nodes,
            self.peak_rss_mb,
            self.last_rss_mb,
            self.rows_written,
            cycles,
        );
        let _ = std::fs::write(SUMMARY_PATH, &summary);
        eprintln!("{}", summary);
    }
}
