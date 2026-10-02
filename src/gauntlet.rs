//! Native visual/scan receipts used by tools/gauntlet.ps1.
use crate::scanner::{self, FileNode, ScanProgress};
use eframe::egui;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const LIVE_FILES: &[&str] = &[
    "01-first-preview.png",
    "02-growing-scan.png",
    "03-paused.png",
    "04-resumed.png",
    "05-canceled.png",
];

pub struct CacheScript {
    pub dir: PathBuf,
    pub stage: usize,
    pub frames: u32,
    pub pending: bool,
    pub entered: Instant,
    pub button_pos: egui::Pos2,
    pub click_frames: u8,
    pub clicked: bool,
}
impl CacheScript {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).expect("create cache capture directory");
        Self { dir, stage: 0, frames: 0, pending: false, entered: Instant::now(),
            button_pos: egui::Pos2::ZERO, click_frames: 0, clicked: false }
    }
}

pub struct LiveScript {
    pub dir: PathBuf,
    pub stage: usize,
    pub entered: Instant,
    pub initialized: bool,
    pub pending: bool,
    pub first_files: u64,
    pub paused_files: u64,
    pub pause_settled: bool,
    pub receipts: Vec<String>,
    pub layout_frames: u64,
    pub layout_updates: u64,
    pub layout_last_files: u64,
}
impl LiveScript {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).expect("create live screenshot directory");
        Self {
            dir,
            stage: 0,
            entered: Instant::now(),
            initialized: false,
            pending: false,
            first_files: 0,
            paused_files: 0,
            pause_settled: false,
            receipts: Vec::new(),
            layout_frames: 0,
            layout_updates: 0,
            layout_last_files: 0,
        }
    }
    pub fn record(&mut self, message: String) {
        eprintln!("[SpaceViewGauntlet] {message}");
        self.receipts.push(message);
        std::fs::write(self.dir.join("live-report.txt"), self.receipts.join("\n"))
            .expect("write live report");
    }
    pub fn capture(&mut self, ctx: &egui::Context) {
        if !self.pending {
            self.pending = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

fn node_count(node: &FileNode) -> usize {
    1 + node.children.iter().map(node_count).sum::<usize>()
}

/// Read-only timing probe against real files, independent of GPU/window startup.
pub fn scan_probe(path: &Path, output: &Path, seconds: f32, cache: bool, previews: bool) {
    std::fs::create_dir_all(output).expect("create probe output");
    let mut log = std::fs::File::create(output.join("scan-probe.csv")).unwrap();
    writeln!(
        log,
        "elapsed_ms,preview_files,preview_bytes,preview_nodes,worker_files"
    )
    .unwrap();
    let progress = Arc::new(ScanProgress::new());
    let p = progress.clone();
    let root = path.to_path_buf();
    let (tx, rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        if cache {
            crate::scan_cache::scan_with_options(&root, p, previews.then_some(tx), true)
        } else {
            scanner::scan_full(&root, p, previews.then_some(tx), false)
        }
    });
    let start = Instant::now();
    let mut first = None;
    let mut count = 0;
    let mut previous_files = 0;
    while start.elapsed().as_secs_f32() < seconds {
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(tree) => {
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                first.get_or_insert(elapsed);
                count += 1;
                let nodes = node_count(&tree);
                assert!(nodes <= scanner::PREVIEW_NODE_BUDGET);
                assert!(
                    tree.file_count >= previous_files,
                    "scan totals moved backwards"
                );
                previous_files = tree.file_count;
                writeln!(
                    log,
                    "{elapsed:.2},{},{},{nodes},{}",
                    tree.file_count,
                    tree.size,
                    progress.files_scanned.load(Ordering::Relaxed)
                )
                .unwrap();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if worker.is_finished() { break; }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    let completed = worker.is_finished();
    if !completed {
        progress.cancel.store(true, Ordering::Relaxed);
    }
    let result = worker.join().expect("scan worker panicked");
    if let Some(root) = result {
        assert_eq!(
            root.file_count,
            progress.files_scanned.load(Ordering::Relaxed)
        );
        assert_eq!(root.size, progress.bytes_scanned.load(Ordering::Relaxed));
    }
    if previews {
        let first = first.expect("no live preview arrived");
        assert!(first < 1000.0, "first live preview took {first:.2}ms");
        assert!(count >= 2, "select a folder large enough for repeated live previews");
    }
    let report = format!("[SpaceViewGauntlet] scan path: {}\n[SpaceViewGauntlet] cache identities: {cache}; previews: {previews}\n[SpaceViewGauntlet] first preview: {:.2} ms\n[SpaceViewGauntlet] live previews: {count}\n[SpaceViewGauntlet] files discovered: {}\n[SpaceViewGauntlet] completed: {completed}\n[SpaceViewGauntlet] COMPLETE scan probe passed\n",
        path.display(), first.unwrap_or(0.0), progress.files_scanned.load(Ordering::Relaxed));
    std::fs::write(output.join("scan-probe.txt"), &report).unwrap();
    eprint!("{report}");
}
