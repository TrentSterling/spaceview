use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
}


pub struct ScanProgress {
    pub files_scanned: AtomicU64,
    pub bytes_scanned: AtomicU64,
    pub cancel: AtomicBool,
    pub scan_start: Instant,
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            files_scanned: AtomicU64::new(0),
            bytes_scanned: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            scan_start: Instant::now(),
        }
    }
}

pub fn scan_directory(root: &Path, progress: Arc<ScanProgress>) -> Option<FileNode> {
    if progress.cancel.load(Ordering::Relaxed) {
        return None;
    }

    let mut node = FileNode {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string()),
        path: root.to_path_buf(),
        size: 0,
        is_dir: true,
        children: Vec::new(),
    };

    let entries: Vec<_> = match std::fs::read_dir(root) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return Some(node),
    };

    for entry in entries {
        if progress.cancel.load(Ordering::Relaxed) {
            return None;
        }

        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            // Skip system/hidden dirs that will just error out
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "System Volume Information" || name == "$Recycle.Bin" {
                continue;
            }
            if let Some(child) = scan_directory(&path, progress.clone()) {
                node.size += child.size;
                if child.size > 0 {
                    node.children.push(child);
                }
            }
        } else {
            let file_size = metadata.len();
            progress.files_scanned.fetch_add(1, Ordering::Relaxed);
            progress.bytes_scanned.fetch_add(file_size, Ordering::Relaxed);

            node.size += file_size;
            node.children.push(FileNode {
                name: entry.file_name().to_string_lossy().to_string(),
                path,
                size: file_size,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }

    // Sort children largest first
    node.children.sort_by(|a, b| b.size.cmp(&a.size));

    Some(node)
}
