//! Verified rescan plans. Cached results are never published before journal validation.
use crate::journal::{self, Change, Checkpoint, Journal};
use crate::scanner::{self, FileNode, ScanProgress};
use bincode::Options;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{mpsc::SyncSender, Arc};

const MAGIC: &[u8; 8] = b"SVSCAN02";
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const CACHE_BUDGET: u64 = 512 * 1024 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    checkpoint: Checkpoint,
    root: PathBuf,
    tree: FileNode,
}

fn restore_paths(
    node: &mut FileNode,
    path: PathBuf,
    depth: usize,
    budget: &mut usize,
) -> io::Result<()> {
    if depth > 512 || *budget == 0 {
        return Err(invalid("cached tree exceeds path budget"));
    }
    *budget -= 1;
    node.path = path;
    for child in &mut node.children {
        let path = node.path.join(&child.name);
        if path.parent() != Some(node.path.as_path())
            || path.file_name().map(|n| n.to_string_lossy()) != Some(child.name.as_str().into())
        {
            return Err(invalid("invalid cached name"));
        }
        restore_paths(child, path, depth + 1, budget)?;
    }
    Ok(())
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_BYTES)
        .reject_trailing_bytes()
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn cache_file(root: &Path) -> Option<PathBuf> {
    let dir = if let Some(path) = std::env::var_os("SPACEVIEW_PREFS_DIR") {
        PathBuf::from(path).join("scan-cache")
    } else {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("SpaceView/scan-cache")
    };
    let key = format!(
        "{:x}",
        Sha256::digest(root.to_string_lossy().to_lowercase().as_bytes())
    );
    Some(dir.join(format!("{key}.svcache")))
}

fn validate_tree(node: &FileNode, root: &Path, depth: usize, budget: &mut usize) -> io::Result<()> {
    if depth > 512 || *budget == 0 || !node.path.starts_with(root) {
        return Err(invalid("invalid cached tree"));
    }
    *budget -= 1;
    if !node.is_dir && (!node.children.is_empty() || node.file_count != 0) {
        return Err(invalid("invalid cached file"));
    }
    let mut names = HashSet::new();
    let mut size = 0u64;
    let mut count = 0u64;
    let mut modified = 0;
    for child in &node.children {
        if child.path.parent() != Some(node.path.as_path())
            || child.path.file_name().map(|n| n.to_string_lossy())
                != Some(child.name.as_str().into())
            || !names.insert(&child.name)
        {
            return Err(invalid("invalid cached path"));
        }
        validate_tree(child, root, depth + 1, budget)?;
        size = size
            .checked_add(child.size)
            .ok_or_else(|| invalid("cached size overflow"))?;
        count = count
            .checked_add(if child.is_dir { child.file_count } else { 1 })
            .ok_or_else(|| invalid("cached count overflow"))?;
        modified = modified.max(child.modified);
    }
    if node.is_dir && (node.size != size || node.file_count != count || node.modified != modified) {
        return Err(invalid("cached totals disagree"));
    }
    Ok(())
}

fn load(path: &Path, root: &Path) -> io::Result<Record> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if !(48..=MAX_BYTES + 48).contains(&length) {
        return Err(invalid("invalid cache length"));
    }
    let mut header = [0u8; 48];
    file.read_exact(&mut header)?;
    if &header[..8] != MAGIC || u64::from_le_bytes(header[8..16].try_into().unwrap()) != length - 48
    {
        return Err(invalid("incompatible cache format"));
    }
    let mut bytes = Vec::with_capacity((length - 48) as usize);
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if Sha256::digest(&bytes).as_slice() != &header[16..48] {
        return Err(invalid("cache checksum failed"));
    }
    let mut record: Record = codec()
        .deserialize(&bytes)
        .map_err(|_| invalid("cache decode failed"))?;
    if record.root != root || !record.tree.is_dir {
        return Err(invalid("cache belongs to another folder"));
    }
    restore_paths(&mut record.tree, root.to_path_buf(), 0, &mut 5_000_000)?;
    validate_tree(&record.tree, root, 0, &mut 5_000_000)?;
    Ok(record)
}

fn save(path: &Path, checkpoint: &Checkpoint, tree: &FileNode) -> io::Result<()> {
    // Serialize references, avoiding a clone of the full metadata tree.
    #[derive(serde::Serialize)]
    struct RefRecord<'a> {
        checkpoint: &'a Checkpoint,
        root: &'a Path,
        tree: &'a FileNode,
    }
    let record = RefRecord {
        checkpoint,
        root: &tree.path,
        tree,
    };
    if codec()
        .serialized_size(&record)
        .map_err(|_| invalid("cache size failed"))?
        > MAX_BYTES
    {
        return Err(invalid("scan exceeds cache budget"));
    }
    let bytes = codec()
        .serialize(&record)
        .map_err(|_| invalid("cache encode failed"))?;
    let dir = path
        .parent()
        .ok_or_else(|| invalid("cache directory missing"))?;
    std::fs::create_dir_all(dir)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let write = || -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(MAGIC)?;
        file.write_all(&(bytes.len() as u64).to_le_bytes())?;
        file.write_all(&Sha256::digest(&bytes))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    };
    if let Err(error) = write() {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    // Bound the entire cache store, evicting only our own complete cache files.
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?;
            if path.extension()?.to_str()? != "svcache"
                || name.len() != 64
                || !name.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            Some((metadata.modified().ok()?, metadata.len(), path))
        })
        .collect();
    files.sort_by_key(|entry| entry.0);
    let mut total: u64 = files.iter().map(|entry| entry.1).sum();
    let mut count = files.len();
    for (_, bytes, candidate) in files {
        if total <= CACHE_BUDGET && count <= 8 {
            break;
        }
        if candidate != path && std::fs::remove_file(candidate).is_ok() {
            total -= bytes;
            count -= 1;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Index {
    dirs: HashMap<u64, PathBuf>,
    parents: HashMap<u64, Vec<u64>>,
    volatile: HashSet<PathBuf>,
}
impl Index {
    fn build(node: &FileNode) -> Self {
        fn walk(node: &FileNode, parent_id: u64, index: &mut Index) {
            if node.is_dir && node.file_id != 0 {
                index.dirs.insert(node.file_id, node.path.clone());
            }
            if node.file_id != 0 && parent_id != 0 {
                let parents = index.parents.entry(node.file_id).or_default();
                if !parents.contains(&parent_id) {
                    parents.push(parent_id);
                }
            }
            if node.volatile || node.file_id == 0 {
                index.volatile.insert(if node.is_dir {
                    node.path.clone()
                } else {
                    node.path.parent().unwrap().to_path_buf()
                });
            }
            for child in &node.children {
                walk(child, node.file_id, index);
            }
        }
        let mut index = Self::default();
        walk(node, 0, &mut index);
        index
    }
    fn dirty(
        &self,
        changes: &[Change],
        root: &Path,
        security_ancestors: Option<&HashSet<u64>>,
    ) -> io::Result<HashSet<PathBuf>> {
        let mut dirty = self.volatile.clone();
        for change in changes {
            // Inheritance can change access without a record for every child.
            // Include ancestors above the selected root and all hardlink IDs.
            // Unknown ancestry requires the conservative volume-wide fallback.
            if change.reason & 0x800 != 0
                && (security_ancestors.is_none_or(|ids| ids.contains(&change.id))
                    || self.dirs.contains_key(&change.id)
                    || self.dirs.contains_key(&change.parent)
                    || self.parents.contains_key(&change.id))
            {
                return Err(invalid("access permissions changed"));
            }
            if let Some(parent) = self.dirs.get(&change.parent) {
                dirty.insert(parent.clone());
            }
            if let Some(parents) = self.parents.get(&change.id) {
                for parent in parents {
                    if let Some(path) = self.dirs.get(parent) {
                        if path.starts_with(root) {
                            dirty.insert(path.clone());
                        }
                    }
                }
            }
            if let Some(dir) = self.dirs.get(&change.id) {
                dirty.insert(dir.clone());
            }
        }
        Ok(dirty)
    }
}

fn ancestors(dirty: &HashSet<PathBuf>, root: &Path) -> HashSet<PathBuf> {
    let mut result = HashSet::new();
    for path in dirty {
        let mut current = Some(path.as_path());
        while let Some(path) = current {
            if !path.starts_with(root) {
                break;
            }
            result.insert(path.to_path_buf());
            current = path.parent();
        }
    }
    result
}

fn refresh(
    mut node: FileNode,
    dirty: &HashSet<PathBuf>,
    ancestors: &HashSet<PathBuf>,
    progress: &Arc<ScanProgress>,
) -> Option<FileNode> {
    if !progress.keep_scanning() {
        return None;
    }
    if !ancestors.contains(&node.path) {
        progress
            .files_reused
            .fetch_add(node.file_count, Ordering::Relaxed);
        return Some(node);
    }
    if !dirty.contains(&node.path) {
        let children = std::mem::take(&mut node.children);
        node.size = 0;
        node.file_count = 0;
        node.modified = 0;
        for child in children {
            if child.is_dir {
                scanner::append_child(&mut node, refresh(child, dirty, ancestors, progress)?);
            } else {
                scanner::append_child(&mut node, child);
                progress.files_reused.fetch_add(1, Ordering::Relaxed);
            }
        }
    } else {
        let stamp = journal::stamp(&node.path).ok();
        if stamp
            .as_ref()
            .is_some_and(|s| s.id != node.file_id || !s.is_dir || s.reparse)
        {
            return scanner::scan_full(&node.path, progress.clone(), None, true);
        }
        progress.directories_read.fetch_add(1, Ordering::Relaxed);
        let old: HashMap<_, _> = std::mem::take(&mut node.children)
            .into_iter()
            .map(|c| (c.name.clone(), c))
            .collect();
        let mut old = old;
        node.size = 0;
        node.file_count = 0;
        node.modified = 0;
        node.volatile = stamp.as_ref().is_none_or(|s| s.volatile);
        let Ok(entries) = std::fs::read_dir(&node.path) else {
            node.volatile = true;
            return Some(node);
        };
        for entry in entries {
            if !progress.keep_scanning() {
                return None;
            }
            let Ok(entry) = entry else {
                node.volatile = true;
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                node.volatile = true;
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir()
                && (name.eq_ignore_ascii_case("System Volume Information")
                    || name.eq_ignore_ascii_case("$Recycle.Bin"))
            {
                continue;
            }
            let stamp = journal::stamp(&path).ok();
            progress.metadata_read.fetch_add(1, Ordering::Relaxed);
            if stamp
                .as_ref()
                .is_some_and(|s| s.reparse || s.is_dir != kind.is_dir())
            {
                node.volatile = true;
                continue;
            }
            if kind.is_dir() {
                let child = match old.remove(&name) {
                    Some(child)
                        if child.is_dir
                            && stamp
                                .as_ref()
                                .is_some_and(|s| s.id != 0 && s.id == child.file_id) =>
                    {
                        refresh(child, dirty, ancestors, progress)?
                    }
                    _ => scanner::scan_full(&path, progress.clone(), None, true)?,
                };
                scanner::append_child(&mut node, child);
            } else if kind.is_file() {
                let Ok(metadata) = entry.metadata() else {
                    node.volatile = true;
                    continue;
                };
                let child = FileNode {
                    name,
                    path,
                    size: stamp.as_ref().map_or(metadata.len(), |s| s.size),
                    is_dir: false,
                    file_count: 0,
                    modified: stamp.as_ref().map_or_else(
                        || {
                            metadata
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map_or(0, |d| d.as_secs())
                        },
                        |s| s.modified,
                    ),
                    children: Vec::new(),
                    file_id: stamp.as_ref().map_or(0, |s| s.id),
                    volatile: stamp.as_ref().is_none_or(|s| s.volatile),
                };
                scanner::append_child(&mut node, child);
            }
        }
    }
    node.children.sort_by(|a, b| b.size.cmp(&a.size));
    Some(node)
}

fn reset_counts(progress: &ScanProgress) {
    for counter in [
        &progress.files_scanned,
        &progress.bytes_scanned,
        &progress.directories_read,
        &progress.metadata_read,
        &progress.files_reused,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
}

/// Journal reasons coalesce while handles stay open, including timestamp-only
/// handles that bypass write sharing. Refresh file metadata along the cached
/// plan before reuse, without rediscovering unchanged folder contents.
fn verify_metadata(node: &mut FileNode, progress: &ScanProgress) -> Option<HashSet<PathBuf>> {
    if !progress.keep_scanning() {
        return None;
    }
    let mut failed = HashSet::new();
    if node.is_dir {
        failed = node
            .children
            .par_iter_mut()
            .map(|child| verify_metadata(child, progress))
            .try_reduce(HashSet::new, |mut paths, next| {
                paths.extend(next);
                Some(paths)
            })?;
        node.size = 0;
        node.file_count = 0;
        node.modified = 0;
        for child in &node.children {
            node.size = node.size.saturating_add(child.size);
            node.file_count =
                node.file_count
                    .saturating_add(if child.is_dir { child.file_count } else { 1 });
            node.modified = node.modified.max(child.modified);
        }
        node.children.sort_by(|a, b| b.size.cmp(&a.size));
    } else {
        progress.metadata_read.fetch_add(1, Ordering::Relaxed);
        match journal::fresh_stamp(&node.path) {
            Ok(stamp)
                if stamp.id == node.file_id && stamp.id != 0 && !stamp.is_dir && !stamp.reparse =>
            {
                node.size = stamp.size;
                node.modified = stamp.modified;
            }
            _ => {
                node.volatile = true;
                failed.insert(node.path.parent().unwrap().to_path_buf());
            }
        }
        progress.files_scanned.fetch_add(1, Ordering::Relaxed);
    }
    Some(failed)
}

fn verify_plan(node: &mut FileNode, progress: &ScanProgress) -> Option<HashSet<PathBuf>> {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    let pool = POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .expect("metadata worker pool")
    });
    pool.install(|| verify_metadata(node, progress))
}

pub fn scan_with_options(
    root: &Path,
    progress: Arc<ScanProgress>,
    snapshots: Option<SyncSender<FileNode>>,
    force: bool,
) -> Option<FileNode> {
    let root = canonical_root(root);
    let path = cache_file(&root);
    scan_at(&root, progress, snapshots, path.as_deref(), force)
}

fn canonical_root(root: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let chars: Vec<_> = canonical.as_os_str().encode_wide().collect();
        let prefix: Vec<_> = r"\\?\".encode_utf16().collect();
        // Keep ordinary drive paths for Explorer, clipboard and recycle actions.
        if chars.starts_with(&prefix) && chars.get(5) == Some(&(b':' as u16)) {
            return PathBuf::from(std::ffi::OsString::from_wide(&chars[4..]));
        }
    }
    canonical
}

fn scan_at(
    root: &Path,
    progress: Arc<ScanProgress>,
    snapshots: Option<SyncSender<FileNode>>,
    cache: Option<&Path>,
    force: bool,
) -> Option<FileNode> {
    if std::env::var_os("SPACEVIEW_DISABLE_CACHE").is_some() {
        return scanner::scan_full(root, progress, snapshots, false);
    }
    progress.phase.store(1, Ordering::Relaxed);
    let journal = Journal::open(root).ok();
    let begin = journal.as_ref().and_then(|j| j.query().ok().map(|q| q.0));
    let root_stamp = journal::stamp(root).ok();
    let mut reason = if force {
        "requested full scan".to_string()
    } else {
        "no verified cache".to_string()
    };
    if !force {
        if let (Some(journal), Some(cache), Some(stamp)) = (&journal, cache, &root_stamp) {
            match load(cache, root) {
                Ok(record)
                    if record.tree.file_id == stamp.id
                        && stamp.id != 0
                        && record.checkpoint.serial == stamp.serial =>
                {
                    let incremental = || -> io::Result<Option<(FileNode, Checkpoint)>> {
                        let mut tree = record.tree;
                        let mut checkpoint = record.checkpoint;
                        let security_ancestors = root
                            .ancestors()
                            .map(|path| journal::stamp(path).map(|s| s.id))
                            .collect::<io::Result<HashSet<_>>>()
                            .ok();
                        // Catch up changes made during validation as well. If the
                        // tree remains busy, fall back to the ordinary live scan.
                        for _ in 0..4 {
                            let (fence, changes) = journal.changes(&checkpoint, &progress)?;
                            let index = Index::build(&tree);
                            let dirty = index.dirty(&changes, root, security_ancestors.as_ref())?;
                            progress.phase.store(2, Ordering::Relaxed);
                            progress.files_reused.store(0, Ordering::Relaxed);
                            if !dirty.is_empty() {
                                let routes = ancestors(&dirty, root);
                                tree = match refresh(tree, &dirty, &routes, &progress) {
                                    Some(tree) => tree,
                                    None => return Ok(None),
                                };
                            }
                            progress.files_scanned.store(0, Ordering::Relaxed);
                            let Some(failed) = verify_plan(&mut tree, &progress) else {
                                return Ok(None);
                            };
                            // This fence precedes fresh reads, so concurrent
                            // changes remain in the next interval, never lost.
                            checkpoint = fence;
                            let (_, changes) = journal.changes(&checkpoint, &progress)?;
                            let mut index = Index::build(&tree);
                            index.volatile.clear();
                            let dirty = index.dirty(&changes, root, security_ancestors.as_ref())?;
                            // Open writers remain volatile even when there are
                            // no more events. They were read in this pass already.
                            if dirty.is_empty() && failed.is_empty() {
                                return Ok(Some((tree, checkpoint)));
                            }
                        }
                        Err(invalid("folder remained busy during refresh"))
                    };
                    match incremental() {
                        Ok(Some((tree, checkpoint))) => {
                            if !progress.keep_scanning() {
                                return None;
                            }
                            progress
                                .files_scanned
                                .store(tree.file_count, Ordering::Relaxed);
                            progress.bytes_scanned.store(tree.size, Ordering::Relaxed);
                            // Entirely unchanged trees still count as reused.
                            if progress.directories_read.load(Ordering::Relaxed) == 0 {
                                progress
                                    .files_reused
                                    .store(tree.file_count, Ordering::Relaxed);
                            }
                            let saved = save(cache, &checkpoint, &tree).is_ok();
                            if !progress.keep_scanning() {
                                return None;
                            }
                            *progress.cache_summary.lock().unwrap() = format!(
                                "Verified refresh: {} folders read, {} files reused{}",
                                progress.directories_read.load(Ordering::Relaxed),
                                progress.files_reused.load(Ordering::Relaxed),
                                if saved { "" } else { " (cache not saved)" }
                            );
                            if let Some(tx) = snapshots {
                                let _ = tx.try_send(scanner::preview_node(
                                    &tree,
                                    scanner::PREVIEW_NODE_BUDGET,
                                ));
                            }
                            return Some(tree);
                        }
                        Ok(None) => return None,
                        Err(error) => reason = error.to_string(),
                    }
                }
                Ok(_) => reason = "selected folder identity changed".into(),
                Err(error) => reason = error.to_string(),
            }
        } else {
            reason = "journal unavailable".into();
        }
    }
    if !progress.keep_scanning() {
        return None;
    }
    reset_counts(&progress);
    progress.phase.store(0, Ordering::Relaxed);
    let tree = scanner::scan_full(root, progress.clone(), snapshots, begin.is_some())?;
    if !progress.keep_scanning() {
        return None;
    }
    // A cold scan is anchored BEFORE traversal, not afterwards. Every change
    // during the scan is replayed the next time this baseline is considered.
    let saved = if let (Some(cache), Some(begin), Some(stamp)) =
        (cache, begin.as_ref(), root_stamp.as_ref())
    {
        tree.file_id == stamp.id && save(cache, begin, &tree).is_ok()
    } else {
        false
    };
    *progress.cache_summary.lock().unwrap() = format!(
        "Full scan{}",
        if saved { "; rescan cache saved" } else { "" }
    );
    eprintln!("[SpaceViewCache] full scan: {reason}; cache_saved={saved}");
    Some(tree)
}

pub fn probe(root: &Path, output: &Path, force: bool, expect_reuse: bool) {
    let progress = Arc::new(ScanProgress::new());
    let tree = scan_with_options(root, progress.clone(), None, force).expect("cache probe scan");
    let summary = progress.cache_summary.lock().unwrap();
    if expect_reuse {
        assert!(
            summary.starts_with("Verified refresh"),
            "cross-process cache failed: {summary}"
        );
        assert!(progress.files_reused.load(Ordering::Relaxed) > 1000);
    }
    let report = format!("PASS elapsed_ms={:.2} folders_read={} metadata_read={} files_reused={} files={} bytes={} {}\nCOMPLETE scan cache probe passed\n",
        progress.scan_start.elapsed().as_secs_f64() * 1000.0,
        progress.directories_read.load(Ordering::Relaxed), progress.metadata_read.load(Ordering::Relaxed),
        progress.files_reused.load(Ordering::Relaxed), tree.file_count, tree.size, summary);
    std::fs::create_dir_all(output).unwrap();
    std::fs::write(output.join("cache-probe.txt"), &report).unwrap();
    eprintln!("{report}");
}

/// A native, non-elevated test against a real NTFS journal and private fixture.
pub fn gauntlet(output: &Path) {
    std::fs::create_dir_all(output).expect("create cache report directory");
    let fixture = output.join(format!("fixture-{}", std::process::id()));
    std::fs::create_dir(&fixture).expect("create exclusive fixture");
    let fixture = std::fs::canonicalize(fixture).unwrap();
    let cache = output.join("fixture.svcache");
    let write = |path: &str, size: u64| {
        let path = fixture.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(path).unwrap().set_len(size).unwrap();
    };
    for dir in 0..40 {
        for file in 0..50 {
            write(&format!("d{dir}/file-{file}.bin"), 64 + file);
        }
    }
    write("nested/deep/original.bin", 100);
    let mut receipts = Vec::new();
    let mut run = |name: &str, force: bool, expect_reuse: bool| -> FileNode {
        let p = Arc::new(ScanProgress::new());
        let tree = scan_at(&fixture, p.clone(), None, Some(&cache), force).expect("fixture scan");
        let elapsed_ms = p.scan_start.elapsed().as_secs_f64() * 1000.0;
        let full = scanner::scan_full(&fixture, Arc::new(ScanProgress::new()), None, true).unwrap();
        fn inventory(node: &FileNode, rows: &mut Vec<(PathBuf, bool, u64, u64, u64)>) {
            rows.push((
                node.path.clone(),
                node.is_dir,
                node.size,
                node.file_count,
                node.modified,
            ));
            for child in &node.children {
                inventory(child, rows);
            }
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        inventory(&tree, &mut a);
        inventory(&full, &mut b);
        a.sort();
        b.sort();
        if a != b {
            let mismatch = a.iter().zip(&b).find(|(cached, fresh)| cached != fresh);
            panic!(
                "{name}: cached/fresh mismatch: {mismatch:?}; cached_rows={} fresh_rows={}",
                a.len(),
                b.len()
            );
        }
        let summary = p.cache_summary.lock().unwrap().clone();
        if expect_reuse {
            assert!(summary.starts_with("Verified refresh"), "{name}: {summary}");
            assert!(p.files_reused.load(Ordering::Relaxed) > 1000);
        } else {
            assert!(
                summary.starts_with("Full scan"),
                "{name}: expected full fallback, got {summary}"
            );
        }
        let message = format!(
            "PASS {name}: elapsed_ms={:.2} directories_read={} metadata_read={} files_reused={} {}",
            elapsed_ms,
            p.directories_read.load(Ordering::Relaxed),
            p.metadata_read.load(Ordering::Relaxed),
            p.files_reused.load(Ordering::Relaxed),
            summary
        );
        eprintln!("{message}");
        receipts.push(message);
        std::fs::write(output.join("cache-report.txt"), receipts.join("\n")).unwrap();
        tree
    };
    run("cold baseline", true, false);
    run("unchanged warm", false, true);
    write("d3/new.bin", 512);
    write("d4/file-3.bin", 9000);
    std::fs::remove_file(fixture.join("d5/file-2.bin")).unwrap();
    std::fs::rename(
        fixture.join("d6/file-1.bin"),
        fixture.join("d7/renamed.bin"),
    )
    .unwrap();
    std::fs::rename(fixture.join("nested"), fixture.join("moved")).unwrap();
    write("new/deep/file.bin", 99);
    run("create resize delete and file/folder moves", false, true);
    std::fs::hard_link(fixture.join("d8/file-0.bin"), fixture.join("d9/linked.bin")).unwrap();
    run("hardlink creation", false, true);
    std::fs::OpenOptions::new()
        .write(true)
        .open(fixture.join("d9/linked.bin"))
        .unwrap()
        .set_len(12345)
        .unwrap();
    run("hardlink resize updates both aliases", false, true);
    let external = output.join("external-link.bin");
    std::fs::hard_link(fixture.join("d8/file-0.bin"), &external).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&external)
        .unwrap()
        .set_len(23456)
        .unwrap();
    run(
        "write through hardlink outside selected folder",
        false,
        true,
    );
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(fixture.join("d10/file-0.bin"))
        .unwrap();
    writer.set_len(500).unwrap();
    run("open writer baseline", true, false);
    writer.set_len(900).unwrap();
    run("open writer first refresh", false, true);
    writer.set_len(1400).unwrap();
    run("open writer repeated reason", false, true);
    drop(writer);
    run("writer closed", false, true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Attribute-only writers bypass sharing and oplock checks. Their
        // repeated timestamp changes can coalesce into one journal reason.
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .access_mode(0x100)
            .open(fixture.join("d12/file-0.bin"))
            .unwrap();
        writer
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000))
            .unwrap();
        run("attribute writer baseline", true, false);
        writer
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_100))
            .unwrap();
        run("attribute-only timestamp change", false, true);
        writer
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_200))
            .unwrap();
        run("repeated attribute-only timestamp change", false, true);
        drop(writer);
    }
    std::fs::write(&cache, b"interrupted or corrupt write").unwrap();
    run("corrupt cache full fallback", false, false);
    let mut bytes = std::fs::read(&cache).unwrap();
    let end = bytes.len() - 1;
    bytes[end] ^= 1;
    std::fs::write(&cache, bytes).unwrap();
    run("checksum mismatch full fallback", false, false);
    let mut record = load(&cache, &fixture).unwrap();
    record.checkpoint.journal_id ^= 1;
    save(&cache, &record.checkpoint, &record.tree).unwrap();
    run("journal reset full fallback", false, false);
    let mut record = load(&cache, &fixture).unwrap();
    record.checkpoint.next = 0;
    save(&cache, &record.checkpoint, &record.tree).unwrap();
    run("journal gap full fallback", false, false);
    let mut record = load(&cache, &fixture).unwrap();
    record.checkpoint.context.push_str("different token");
    save(&cache, &record.checkpoint, &record.tree).unwrap();
    run("security context full fallback", false, false);
    #[cfg(windows)]
    {
        let account = format!(
            "{}\\{}:(F)",
            std::env::var("USERDOMAIN").unwrap(),
            std::env::var("USERNAME").unwrap()
        );
        let result = std::process::Command::new("icacls")
            .arg(fixture.join("d11/file-0.bin"))
            .arg("/grant:r")
            .arg(account)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run private fixture ACL change");
        assert!(result.success(), "private fixture ACL update failed");
        run("real access-permission change full fallback", false, false);
    }
    // A replaced folder at the same name must never match the old root ID.
    let moved = output.join(format!("old-root-{}", std::process::id()));
    std::fs::rename(&fixture, &moved).unwrap();
    std::fs::create_dir(&fixture).unwrap();
    write("replacement.bin", 77);
    run("same-path folder replacement full fallback", false, false);
    std::fs::remove_file(fixture.join("replacement.bin")).unwrap();
    std::fs::remove_dir(&fixture).unwrap();
    std::fs::rename(&moved, &fixture).unwrap();
    run("restored original baseline", true, false);
    drop(run);
    let before = std::fs::read(&cache).unwrap();
    let p = Arc::new(ScanProgress::new());
    p.paused.store(true, Ordering::Relaxed);
    let worker_p = p.clone();
    let worker_root = fixture.clone();
    let worker_cache = cache.clone();
    let worker = std::thread::spawn(move || {
        scan_at(&worker_root, worker_p, None, Some(&worker_cache), false)
    });
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(!worker.is_finished());
    p.cancel.store(true, Ordering::Relaxed);
    assert!(worker.join().unwrap().is_none());
    assert_eq!(
        std::fs::read(&cache).unwrap(),
        before,
        "cancellation rewrote the cache"
    );
    receipts
        .push("PASS paused warm cancellation: no result published and baseline unchanged".into());
    std::fs::write(
        output.join("fixture-path.txt"),
        fixture.to_string_lossy().as_bytes(),
    )
    .unwrap();
    receipts.push("COMPLETE verified scan cache checks passed".into());
    let report = receipts.join("\n");
    std::fs::write(output.join("cache-report.txt"), &report).unwrap();
    eprintln!("COMPLETE verified scan cache checks passed");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_paths_restore_unicode_and_reject_escaping_names() {
        let path = PathBuf::from("root").join("long-parent-name-".repeat(30));
        let mut tree = scanner::directory_node(&path);
        scanner::append_child(
            &mut tree,
            FileNode {
                name: "日本語.bin".into(),
                path: path.join("日本語.bin"),
                size: 7,
                is_dir: false,
                file_count: 0,
                modified: 100,
                children: Vec::new(),
                file_id: 9,
                volatile: false,
            },
        );
        let bytes = codec().serialize(&tree).unwrap();
        assert!(bytes.len() < 700, "absolute parent paths were repeated");
        let mut decoded: FileNode = codec().deserialize(&bytes).unwrap();
        assert!(decoded.path.as_os_str().is_empty());
        restore_paths(&mut decoded, path.clone(), 0, &mut 10).unwrap();
        validate_tree(&decoded, &path, 0, &mut 10).unwrap();
        assert_eq!(decoded.children[0].path, tree.children[0].path);
        assert_eq!(decoded.children[0].file_id, 9);
        decoded.children[0].name = "../escape.bin".into();
        assert!(restore_paths(&mut decoded, path, 0, &mut 10).is_err());
    }
    #[test]
    fn dirty_plan_routes_moves_hardlinks_and_permissions() {
        let root = PathBuf::from("root");
        let mut index = Index::default();
        index.dirs.insert(1, root.clone());
        index.dirs.insert(2, root.join("a"));
        index.dirs.insert(3, root.join("b"));
        index.parents.insert(10, vec![2, 3]);
        let dirty = index
            .dirty(
                &[Change {
                    id: 10,
                    parent: 2,
                    reason: 1,
                }],
                &root,
                None,
            )
            .unwrap();
        assert!(dirty.contains(&root.join("a")));
        assert!(dirty.contains(&root.join("b")));
        assert!(ancestors(&dirty, &root).contains(&root));
        assert!(index
            .dirty(
                &[Change {
                    id: 999,
                    parent: 999,
                    reason: 0x800
                }],
                &root,
                None,
            )
            .is_err());
        let dirty = index
            .dirty(
                &[Change {
                    id: 999,
                    parent: 2,
                    reason: 0x100,
                }],
                &root,
                None,
            )
            .unwrap();
        assert_eq!(dirty, HashSet::from([root.join("a")]));
        let scope = HashSet::from([4]);
        assert!(index
            .dirty(
                &[Change {
                    id: 99,
                    parent: 98,
                    reason: 0x800
                }],
                &root,
                Some(&scope)
            )
            .unwrap()
            .is_empty());
        for id in [1, 2, 4, 10] {
            assert!(index
                .dirty(
                    &[Change {
                        id,
                        parent: 98,
                        reason: 0x800
                    }],
                    &root,
                    Some(&scope)
                )
                .is_err());
        }
    }
}
