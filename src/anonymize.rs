//! Anonymized real scan for screenshots.
//!
//! `--shots DIR --scan C:\` scans a real drive with the normal scanner and then
//! rewrites every name in memory before a single frame is drawn. Sizes, file
//! counts and dates are kept (that is what makes a treemap look real); names are
//! kept only when they are well-known system, vendor or tooling folders, version
//! numbers, or hash-like cache names. Everything else (user names, project
//! names, document and media file names) becomes a deterministic fake word, so
//! the same original always maps to the same fake. Extensions are preserved.

use crate::scanner::FileNode;

const KEEP: &[&str] = &[
    "windows", "system32", "syswow64", "winsxs", "servicing", "installer", "softwaredistribution", "assembly", "fonts", "temp", "tmp", "logs", "log", "prefetch", "recovery", "system volume information", "$recycle.bin", "boot", "perflogs",
    "program files", "program files (x86)", "programdata", "common files", "windowsapps", "packages", "package cache", "microsoft", "microsoft.net", "dotnet", "sdk", "shared", "intel", "amd", "nvidia", "nvidia corporation", "realtek", "dell", "hp", "lenovo", "asus", "msi", "razer", "logitech", "corsair",
    "users", "public", "default", "appdata", "local", "locallow", "roaming", "desktop", "documents", "downloads", "pictures", "videos", "music", "saved games", "onedrive", "camera roll", "screenshots", "captures",
    "steam", "steamapps", "common", "workshop", "content", "shadercache", "downloading", "epic games", "launcher", "oculus", "meta", "software", "riot games", "battle.net", "gog galaxy", "games", "ubisoft", "ea games", "origin", "xbox games",
    "unity", "hub", "editor", "data", "playbackengines", "unity projects", "assets", "library", "packagecache", "artifacts", "bee", "projectsettings", "usersettings", "obj", "builds", "build", "unreal projects", "unrealengine", "saved", "intermediate", "deriveddatacache", "binaries", "engine", "plugins", "thirdparty", "source",
    "node_modules", "dist", "src", "target", "release", "debug", "deps", "incremental", ".cargo", "registry", "cache", ".rustup", "toolchains", ".nuget", ".gradle", ".android", ".jdks", ".m2", ".npm", ".pnpm", ".yarn", ".vscode", "extensions", ".dotnet", "tools", "bin", "lib", "include", "site-packages", "python", "python312", "python311", "scripts", "pip", "wheels", "venv", ".venv", "env", "__pycache__", "go", "pkg", "mod", "jetbrains", "rider", "idea", "adobe", "blender foundation", "blender", "ollama", "models", "blobs", "manifests", "docker", ".docker", "wsl", "virtualbox vms", "vmware",
    "google", "chrome", "user data", "code cache", "gpucache", "service worker", "cachestorage", "discord", "slack", "spotify", "obs-studio", "obs", "code", "vs code", "visual studio", "git", "github", "githubdesktop", "gh", "npm", "npm-cache", "yarn", "electron", "firefox", "mozilla", "profiles", "edge", "brave",
    "pagefile.sys", "hiberfil.sys", "swapfile.sys", "dumpstack.log.tmp", "<free space>", "sourcemods", "ndk", "platform-tools", "cmake", "llvm", "rust", "cargo", "java", "jdk", "jre", "nodejs", "node", "ffmpeg", "sox", "espeak ng",
];

/// Neutral words for fake names (deterministic from a hash of the original).
const WORDS: &[&str] = &[
    "amber", "basalt", "cobalt", "delta", "ember", "falcon", "granite", "harbor", "iris", "juniper", "kestrel", "lumen", "maple", "nimbus", "onyx", "pebble", "quartz", "raven", "saffron", "tundra", "umber", "velvet", "willow", "xenon", "yarrow", "zephyr",
    "arbor", "birch", "cinder", "dune", "echo", "fable", "glacier", "heron", "inlet", "jasper", "kelp", "lagoon", "meadow", "nectar", "orbit", "prairie", "quill", "ridge", "summit", "thistle", "upland", "vortex", "wren", "yonder", "zenith",
    "atlas", "bramble", "canyon", "drift", "estuary", "fjord", "grove", "hollow", "isle", "jetty", "knoll", "lantern", "marsh", "nook", "oasis", "pinnacle", "quarry", "rapids", "sable", "terrace", "utopia", "vale", "warren", "yew", "zinc",
];

fn hash_name(s: &str) -> u64 {
    // FNV-1a over the lowercase bytes: stable across runs, no crate needed.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn looks_like_version_or_hash(stem: &str) -> bool {
    // "6000.2.4f1", "2022.3.62f1", "1.2.3", "f_000123", "a3f9c2...", GUIDs.
    let s = stem.trim_start_matches("f_");
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, '.' | '-' | '_'))
        && s.chars().any(|c| c.is_ascii_digit())
}

fn fake_stem(original: &str) -> String {
    let h = hash_name(original);
    let a = WORDS[(h % WORDS.len() as u64) as usize];
    let b = WORDS[((h >> 20) % WORDS.len() as u64) as usize];
    let n = (h >> 40) % 100;
    match h % 3 {
        0 => a.to_string(),
        1 => format!("{}-{}", a, b),
        _ => format!("{}_{:02}", a, n),
    }
}

/// Keep, or replace with a fake that keeps the extension.
fn anonymize_name(name: &str, is_dir: bool) -> String {
    let lower = name.to_lowercase();
    if KEEP.contains(&lower.as_str()) {
        return name.to_string();
    }
    let split = if is_dir { None } else { name.rfind('.') };
    let (stem, ext) = match split {
        Some(i) if i > 0 && name.len() - i <= 10 && name[i + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    if looks_like_version_or_hash(stem) {
        return name.to_string();
    }
    format!("{}{}", fake_stem(stem), ext.to_lowercase())
}

/// Rewrite every name in the tree and rebuild paths. Sizes, counts and dates
/// are untouched. Returns the sorted set of names that were kept verbatim so a
/// human can check the allowlist did not let anything personal through.
pub fn anonymize(root: &mut FileNode) -> Vec<String> {
    fn walk(node: &mut FileNode, parent: &std::path::Path, kept: &mut std::collections::BTreeSet<String>) {
        let new_name = anonymize_name(&node.name, node.is_dir);
        if new_name == node.name {
            kept.insert(node.name.clone());
        }
        node.name = new_name;
        node.path = parent.join(&node.name);
        let path = node.path.clone();
        for c in &mut node.children {
            walk(c, &path, kept);
        }
    }
    let mut kept = std::collections::BTreeSet::new();
    // The root keeps its drive label ("C:\") so the status bar reads naturally.
    let root_path = root.path.clone();
    for c in &mut root.children {
        walk(c, &root_path, &mut kept);
    }
    kept.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_system_and_versions_replaces_the_rest() {
        assert_eq!(anonymize_name("Windows", true), "Windows");
        assert_eq!(anonymize_name("6000.2.4f1", true), "6000.2.4f1");
        assert_eq!(anonymize_name("f_00012a", false), "f_00012a");
        let a = anonymize_name("trent", true);
        assert_ne!(a, "trent");
        assert_eq!(a, anonymize_name("Trent", true));
        let f = anonymize_name("my tax return 2025.pdf", false);
        assert!(f.ends_with(".pdf") && !f.contains("tax"));
    }
}
