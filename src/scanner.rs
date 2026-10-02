use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FileNode {
    pub name: String,
    #[serde(skip)]
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub file_count: u64,
    pub modified: u64,
    pub children: Vec<FileNode>,
    pub file_id: u64,
    pub volatile: bool,
}

/// Get free space for the drive containing `path`.
pub fn get_free_space(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // canonicalize adds a \\?\ prefix on Windows, breaking starts_with.
    let mut best: Option<(usize, u64)> = None;
    for disk in disks.list() {
        if path.starts_with(disk.mount_point()) {
            let len = disk.mount_point().to_string_lossy().len();
            if best.is_none_or(|(best_len, _)| len > best_len) {
                best = Some((len, disk.available_space()));
            }
        }
    }
    best.map(|(_, space)| space)
}

pub struct ScanProgress {
    pub files_scanned: AtomicU64,
    pub bytes_scanned: AtomicU64,
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
    pub scan_start: Instant,
    pub directories_read: AtomicU64,
    pub metadata_read: AtomicU64,
    pub files_reused: AtomicU64,
    pub phase: AtomicU64,
    pub cache_summary: std::sync::Mutex<String>,
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            files_scanned: AtomicU64::new(0),
            bytes_scanned: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            scan_start: Instant::now(),
            directories_read: AtomicU64::new(0),
            metadata_read: AtomicU64::new(0),
            files_reused: AtomicU64::new(0),
            phase: AtomicU64::new(0),
            cache_summary: std::sync::Mutex::new(String::new()),
        }
    }

    pub(crate) fn keep_scanning(&self) -> bool {
        while self.paused.load(Ordering::Relaxed) {
            if self.cancel.load(Ordering::Relaxed) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        !self.cancel.load(Ordering::Relaxed)
    }
}

const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);
pub const PREVIEW_NODE_BUDGET: usize = 16_384;
const PREVIEW_CHILD_CAP: usize = 512;

pub(crate) fn directory_node(path: &Path) -> FileNode {
    FileNode {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                let display = path.to_string_lossy();
                display
                    .strip_prefix("\\\\?\\")
                    .unwrap_or(&display)
                    .to_string()
            }),
        path: path.to_path_buf(),
        size: 0,
        is_dir: true,
        file_count: 0,
        modified: 0,
        children: Vec::new(),
        file_id: 0,
        volatile: true,
    }
}

pub(crate) fn append_child(parent: &mut FileNode, child: FileNode) {
    parent.size += child.size;
    parent.file_count += if child.is_dir { child.file_count } else { 1 };
    parent.modified = parent.modified.max(child.modified);
    parent.children.push(child);
}

/// Copy bounded detail without walking/sorting millions of discovered files.
/// Full totals survive; WorldLayout aggregates omitted bytes without file actions.
pub(crate) fn preview_node(node: &FileNode, budget: usize) -> FileNode {
    let mut preview = node.clone_shallow();
    let count = node
        .children
        .len()
        .min(PREVIEW_CHILD_CAP)
        .min(budget.saturating_sub(1));
    if count > 0 {
        let child_budget = (budget - 1) / count;
        preview.children = node
            .children
            .iter()
            .take(count)
            .map(|child| preview_node(child, child_budget))
            .collect();
        preview.children.sort_by(|a, b| b.size.cmp(&a.size));
    }
    preview
}

impl FileNode {
    fn clone_shallow(&self) -> Self {
        Self {
            name: self.name.clone(),
            path: self.path.clone(),
            size: self.size,
            is_dir: self.is_dir,
            file_count: self.file_count,
            modified: self.modified,
            children: Vec::new(),
            file_id: self.file_id,
            volatile: self.volatile,
        }
    }
}

struct Scanner {
    progress: Arc<ScanProgress>,
    snapshots: Option<SyncSender<FileNode>>,
    // Completed children of every open ancestor plus the growing active branch.
    stack: Vec<FileNode>,
    last_snapshot: Option<Instant>,
    interval: Duration,
    identify: bool,
}

impl Scanner {
    fn new(progress: Arc<ScanProgress>, snapshots: Option<SyncSender<FileNode>>) -> Self {
        Self {
            progress,
            snapshots,
            stack: Vec::new(),
            last_snapshot: None,
            interval: SNAPSHOT_INTERVAL,
            identify: false,
        }
    }

    fn publish(&mut self) {
        let Some(tx) = self.snapshots.as_ref() else {
            return;
        };
        if self.progress.files_scanned.load(Ordering::Relaxed) == 0
            || self
                .last_snapshot
                .is_some_and(|last| last.elapsed() < self.interval)
        {
            return;
        }
        let level_budget = (PREVIEW_NODE_BUDGET / self.stack.len().max(1)).max(1);
        let mut active: Option<FileNode> = None;
        for node in self.stack.iter().rev() {
            let mut preview = preview_node(node, level_budget);
            if let Some(child) = active.take() {
                append_child(&mut preview, child);
                preview.children.sort_by(|a, b| b.size.cmp(&a.size));
            }
            active = Some(preview);
        }
        if let Some(root) = active {
            // One queued preview, no blocking/backlog when the window is busy.
            // The unabridged final tree uses its own completion channel.
            let _ = tx.try_send(root);
            self.last_snapshot = Some(Instant::now());
        }
    }

    fn directory(&mut self, root: &Path) -> Option<FileNode> {
        if !self.progress.keep_scanning() {
            return None;
        }
        let mut node = directory_node(root);
        if self.identify {
            if let Ok(stamp) = crate::journal::stamp(root) {
                if stamp.reparse || !stamp.is_dir {
                    return Some(node);
                }
                node.file_id = stamp.id;
                node.volatile = stamp.volatile;
            }
        }
        self.stack.push(node);
        self.progress
            .directories_read
            .fetch_add(1, Ordering::Relaxed);
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries {
                let Ok(entry) = entry else {
                    self.stack.last_mut().unwrap().volatile = true;
                    continue;
                };
                if !self.progress.keep_scanning() {
                    return None;
                }
                let Ok(kind) = entry.file_type() else {
                    self.stack.last_mut().unwrap().volatile = true;
                    continue;
                };
                // Avoid symlink/junction cycles and escaping the selected drive.
                if kind.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if kind.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.eq_ignore_ascii_case("System Volume Information")
                        || name.eq_ignore_ascii_case("$Recycle.Bin")
                    {
                        continue;
                    }
                    let child = self.directory(&path)?;
                    append_child(self.stack.last_mut().unwrap(), child);
                } else if kind.is_file() {
                    let stamp = if self.identify {
                        crate::journal::stamp(&path).ok()
                    } else {
                        None
                    };
                    if stamp.as_ref().is_some_and(|s| s.reparse || s.is_dir) {
                        self.stack.last_mut().unwrap().volatile = true;
                        continue;
                    }
                    let Ok(metadata) = entry.metadata() else {
                        self.stack.last_mut().unwrap().volatile = true;
                        continue;
                    };
                    self.progress.metadata_read.fetch_add(1, Ordering::Relaxed);
                    let size = stamp.as_ref().map_or(metadata.len(), |s| s.size);
                    let modified = stamp.as_ref().map(|s| s.modified).unwrap_or_else(|| {
                        metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    });
                    self.progress.files_scanned.fetch_add(1, Ordering::Relaxed);
                    self.progress
                        .bytes_scanned
                        .fetch_add(size, Ordering::Relaxed);
                    append_child(
                        self.stack.last_mut().unwrap(),
                        FileNode {
                            name: entry.file_name().to_string_lossy().into_owned(),
                            path,
                            size,
                            is_dir: false,
                            file_count: 0,
                            modified,
                            children: Vec::new(),
                            file_id: stamp.as_ref().map_or(0, |s| s.id),
                            volatile: stamp.as_ref().is_none_or(|s| s.volatile),
                        },
                    );
                }
                self.publish();
            }
        } else {
            self.stack.last_mut().unwrap().volatile = true;
        }
        let mut node = self.stack.pop().unwrap();
        node.children.sort_by(|a, b| b.size.cmp(&a.size));
        Some(node)
    }
}

pub(crate) fn scan_full(
    root: &Path,
    progress: Arc<ScanProgress>,
    snapshots: Option<SyncSender<FileNode>>,
    identify: bool,
) -> Option<FileNode> {
    let mut scanner = Scanner::new(progress, snapshots);
    scanner.identify = identify;
    scanner.directory(root)
}

/// First discovered file immediately, then at most four updates/second,
/// including progress inside unfinished top-level folders.
pub fn scan_directory_live(
    root: &Path,
    progress: Arc<ScanProgress>,
    snapshots: SyncSender<FileNode>,
) -> Option<FileNode> {
    Scanner::new(progress, Some(snapshots)).directory(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "spaceview-scan-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, path: &str, size: usize) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![7u8; size]).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn count_nodes(node: &FileNode) -> usize {
        1 + node.children.iter().map(count_nodes).sum::<usize>()
    }

    #[test]
    fn publishes_inside_first_directory_before_it_finishes() {
        let f = Fixture::new();
        for i in 0..100 {
            f.file(&format!("one-large-folder/deep/file-{i}.bin"), i + 1);
        }
        let p = Arc::new(ScanProgress::new());
        let (tx, rx) = mpsc::sync_channel(256);
        let mut scanner = Scanner::new(p.clone(), Some(tx));
        scanner.interval = Duration::ZERO;
        let tree = scanner.directory(&f.0).unwrap();
        let previews: Vec<_> = rx.try_iter().collect();
        assert_eq!(previews.first().unwrap().file_count, 1);
        assert_eq!(
            previews.first().unwrap().children[0].name,
            "one-large-folder"
        );
        assert!(previews
            .iter()
            .any(|s| s.file_count > 1 && s.file_count < tree.file_count));
        assert_eq!(tree.file_count, 100);
        assert_eq!(tree.size, 5050);
        assert_eq!(p.bytes_scanned.load(Ordering::Relaxed), tree.size);
        assert_eq!(p.files_scanned.load(Ordering::Relaxed), tree.file_count);
        for s in previews {
            assert!(s.size <= tree.size);
        }
    }

    #[test]
    fn full_queue_never_blocks_scan_or_loses_final_tree() {
        let f = Fixture::new();
        for i in 0..32 {
            f.file(&format!("folder/{i}.bin"), 9);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let mut scanner = Scanner::new(Arc::new(ScanProgress::new()), Some(tx));
        scanner.interval = Duration::ZERO;
        let tree = scanner.directory(&f.0).unwrap();
        assert_eq!(rx.try_iter().count(), 1);
        assert_eq!(tree.file_count, 32);
        assert_eq!(tree.size, 288);
        assert_eq!(tree.children[0].children.len(), 32);
    }

    #[test]
    fn preview_budget_preserves_totals_and_active_branch() {
        let mut scanner = Scanner::new(Arc::new(ScanProgress::new()), None);
        scanner
            .stack
            .push(crate::stress::generate_synthetic_tree(100_000));
        let mut active = directory_node(Path::new("active-folder"));
        active.size = 100;
        active.file_count = 1;
        scanner.stack.push(active);
        scanner.progress.files_scanned.store(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel(1);
        scanner.snapshots = Some(tx);
        let expected_size = scanner.stack[0].size + 100;
        scanner.publish();
        let preview = rx.try_recv().unwrap();
        assert!(count_nodes(&preview) <= PREVIEW_NODE_BUDGET);
        assert_eq!(preview.size, expected_size);
        assert!(preview.children.iter().any(|c| c.name == "active-folder"));
    }

    #[test]
    fn pause_resume_and_cancel_while_paused() {
        let f = Fixture::new();
        f.file("nested/test.bin", 42);
        let p = Arc::new(ScanProgress::new());
        p.paused.store(true, Ordering::Relaxed);
        let worker_p = p.clone();
        let path = f.0.clone();
        let worker = std::thread::spawn(move || Scanner::new(worker_p, None).directory(&path));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(p.files_scanned.load(Ordering::Relaxed), 0);
        assert!(!worker.is_finished());
        p.paused.store(false, Ordering::Relaxed);
        assert_eq!(worker.join().unwrap().unwrap().size, 42);
        let p = Arc::new(ScanProgress::new());
        p.paused.store(true, Ordering::Relaxed);
        let worker_p = p.clone();
        let path = f.0.clone();
        let worker = std::thread::spawn(move || Scanner::new(worker_p, None).directory(&path));
        p.cancel.store(true, Ordering::Relaxed);
        assert!(worker.join().unwrap().is_none());
    }

    #[test]
    fn zero_bytes_empty_dirs_and_case_insensitive_system_skips() {
        let f = Fixture::new();
        f.file("empty.txt", 0);
        f.file("$RECYCLE.BIN/ignored.dat", 100);
        f.file("system volume information/ignored.dat", 100);
        std::fs::create_dir(f.0.join("empty-directory")).unwrap();
        let tree = Scanner::new(Arc::new(ScanProgress::new()), None)
            .directory(&f.0)
            .unwrap();
        assert_eq!(tree.size, 0);
        assert_eq!(tree.file_count, 1);
        assert_eq!(tree.children.len(), 2);
    }
}
