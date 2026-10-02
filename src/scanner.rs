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

pub(crate) fn read_entries(root: &Path, identify: bool)
    -> std::io::Result<Box<dyn Iterator<Item = std::io::Result<FileNode>>>> {
    #[cfg(windows)]
    if identify {
        if let Ok(entries) = crate::journal::directory_entries(root) {
            let root = root.to_path_buf();
            return Ok(Box::new(entries.filter_map(move |entry| {
                let entry = match entry { Ok(entry) => entry, Err(error) => return Some(Err(error)) };
                if entry.stamp.reparse { return None; }
                let path = root.join(&entry.name);
                let name = entry.name.to_string_lossy().into_owned();
                let stamp = entry.stamp;
                Some(Ok(FileNode { name, path, size: if stamp.is_dir { 0 } else { stamp.size },
                    is_dir: stamp.is_dir, file_count: 0, modified: if stamp.is_dir { 0 } else { stamp.modified },
                    children: Vec::new(), file_id: stamp.id, volatile: stamp.id == 0 }))
            })));
        }
    }
    Ok(Box::new(std::fs::read_dir(root)?.filter_map(move |entry| {
        let entry = match entry { Ok(entry) => entry, Err(error) => return Some(Err(error)) };
        let kind = match entry.file_type() { Ok(kind) => kind, Err(error) => return Some(Err(error)) };
        if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) { return None; }
        let path = entry.path();
        let stamp = identify.then(|| crate::journal::fresh_stamp(&path).ok()).flatten();
        if stamp.as_ref().is_some_and(|s| s.reparse || s.is_dir != kind.is_dir()) { return None; }
        let (size, modified) = if kind.is_dir() { (0, 0) }
            else if let Some(stamp) = &stamp { (stamp.size, stamp.modified) }
            else {
                let metadata = match entry.metadata() { Ok(metadata) => metadata, Err(error) => return Some(Err(error)) };
                (metadata.len(), metadata.modified().ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()))
            };
        Some(Ok(FileNode { name: entry.file_name().to_string_lossy().into_owned(), path,
            size, is_dir: kind.is_dir(), file_count: 0, modified, children: Vec::new(),
            file_id: stamp.as_ref().map_or(0, |s| s.id), volatile: stamp.as_ref().is_none_or(|s| s.volatile) }))
    })))
}

/// Copy bounded detail without walking/sorting millions of discovered files.
/// Full totals survive; WorldLayout aggregates omitted bytes without file actions.
pub(crate) fn preview_node(node: &FileNode, budget: usize) -> FileNode {
    preview_branch(node, &[], budget)
}

fn preview_branch(node: &FileNode, active: &[FileNode], budget: usize) -> FileNode {
    let mut preview = node.clone_shallow();
    for branch in active {
        preview.size += branch.size;
        preview.file_count += branch.file_count;
        preview.modified = preview.modified.max(branch.modified);
    }
    // Keep one global budget, independent of the active directory's depth.
    // Reserve the active spine, then give completed branches detail in
    // proportion to their displayed area rather than their child count.
    let active_min = if active.is_empty() { 0 }
        else { (active.len() + 1).min(budget.saturating_sub(1)) };
    let count = node
        .children
        .len()
        .min(PREVIEW_CHILD_CAP)
        .min(budget.saturating_sub(1 + active_min));
    let active_size: u64 = active.iter().map(|branch| branch.size).sum();
    let weight: u128 = node.children.iter().take(count).map(|child| child.size as u128).sum::<u128>()
        + active_size as u128;
    let spare = budget.saturating_sub(1 + count + active_min);
    let share = |size: u64| {
        if weight == 0 { spare / (count + usize::from(active_min > 0)).max(1) }
        else { ((size as u128 * spare as u128) / weight) as usize }
    };
    preview.children = node.children.iter().take(count)
        .map(|child| preview_node(child, 1 + share(child.size))).collect();
    if active_min > 0 {
        preview.children.push(preview_branch(&active[0], &active[1..], active_min + share(active_size)));
    }
    preview.children.sort_by(|a, b| b.size.cmp(&a.size));
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
        if let Some((root, active)) = self.stack.split_first() {
            let root = preview_branch(root, active, PREVIEW_NODE_BUDGET);
            // One queued preview, no blocking/backlog when the window is busy.
            // The unabridged final tree uses its own completion channel.
            let _ = tx.try_send(root);
            self.last_snapshot = Some(Instant::now());
        }
    }

    fn directory(&mut self, root: &Path) -> Option<FileNode> {
        self.directory_with_id(root, 0)
    }

    fn directory_with_id(&mut self, root: &Path, file_id: u64) -> Option<FileNode> {
        if !self.progress.keep_scanning() {
            return None;
        }
        let mut node = directory_node(root);
        node.file_id = file_id;
        node.volatile = file_id == 0;
        if self.identify && file_id == 0 {
            if let Ok(stamp) = crate::journal::fresh_stamp(root) {
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
        if let Ok(entries) = read_entries(root, self.identify) {
            for entry in entries {
                let Ok(entry) = entry else {
                    self.stack.last_mut().unwrap().volatile = true;
                    continue;
                };
                if !self.progress.keep_scanning() {
                    return None;
                }
                if entry.is_dir {
                    let name = &entry.name;
                    if name.eq_ignore_ascii_case("System Volume Information")
                        || name.eq_ignore_ascii_case("$Recycle.Bin")
                    {
                        continue;
                    }
                    let child = self.directory_with_id(&entry.path, entry.file_id)?;
                    append_child(self.stack.last_mut().unwrap(), child);
                } else {
                    self.progress.metadata_read.fetch_add(1, Ordering::Relaxed);
                    self.progress.files_scanned.fetch_add(1, Ordering::Relaxed);
                    self.progress
                        .bytes_scanned
                        .fetch_add(entry.size, Ordering::Relaxed);
                    append_child(self.stack.last_mut().unwrap(), entry);
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
    #[cfg(windows)]
    fn native_directory_pages_keep_all_names_sizes_and_file_ids() {
        let fixture = Fixture::new();
        for i in 0..700 {
            fixture.file(&format!("unicode-\u{1f680}-{i:04}-long-enough-for-multiple-directory-pages.bin"), i % 11);
        }
        std::fs::create_dir(fixture.0.join("nested")).unwrap();
        let entries = read_entries(&fixture.0, true).unwrap().collect::<std::io::Result<Vec<_>>>().unwrap();
        assert_eq!(entries.len(), 701);
        assert!(entries.iter().all(|entry| entry.file_id != 0));
        let mut native: Vec<_> = entries.iter().map(|entry| (entry.name.clone(), entry.is_dir, entry.size)).collect();
        let mut ordinary: Vec<_> = read_entries(&fixture.0, false).unwrap()
            .map(|entry| { let entry = entry.unwrap(); (entry.name, entry.is_dir, entry.size) }).collect();
        native.sort(); ordinary.sort();
        assert_eq!(native, ordinary);
        for entry in entries.iter().step_by(67) {
            assert_eq!(entry.file_id, crate::journal::fresh_stamp(&entry.path).unwrap().id);
        }
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
    fn completed_detail_does_not_collapse_when_active_scan_goes_deeper() {
        let mut completed = crate::stress::generate_synthetic_tree(1000);
        completed.name = "completed".into();
        let expected_detail = count_nodes(&preview_node(&completed, PREVIEW_NODE_BUDGET));
        assert!(expected_detail > 500, "fixture must expose detail-budget shrinkage");
        let mut root = directory_node(Path::new("root"));
        append_child(&mut root, completed);
        for depth in [1, 8, 32] {
            let mut scanner = Scanner::new(Arc::new(ScanProgress::new()), None);
            scanner.stack.push(root.clone());
            for level in 0..depth {
                scanner.stack.push(directory_node(Path::new(&format!("active-{level}"))));
            }
            let leaf = scanner.stack.last_mut().unwrap();
            leaf.size = 1;
            leaf.file_count = 1;
            let (tx, rx) = mpsc::sync_channel(1);
            scanner.snapshots = Some(tx);
            scanner.progress.files_scanned.store(root.file_count + 1, Ordering::Relaxed);
            scanner.publish();
            let preview = rx.try_recv().unwrap();
            let retained = preview.children.iter().find(|child| child.name == "completed").unwrap();
            assert_eq!(count_nodes(retained), expected_detail,
                "completed folder lost visible detail at active depth {depth}");
            assert!(count_nodes(&preview) <= PREVIEW_NODE_BUDGET);
            assert_eq!(preview.size, root.size + 1);
            assert_eq!(preview.file_count, root.file_count + 1);
        }
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
