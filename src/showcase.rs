//! A believable fake drive for screenshots (`--shots`). Nothing here is read
//! from disk: folder names, sizes and dates are invented so the landing page
//! never shows anyone's real files, while still looking like a real machine
//! (a Steam library, Unity projects, videos, a node_modules or two, Windows).

use crate::scanner::FileNode;
use crate::stress::Rng;
use std::path::PathBuf;

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;
const KB: u64 = 1024;
const DAY: u64 = 86_400;
/// Fixed "now" so the age map is stable between runs (2026-09-19).
const NOW: u64 = 1_789_776_000;

struct Gen {
    rng: Rng,
}

impl Gen {
    fn size(&mut self, lo: u64, hi: u64) -> u64 {
        self.rng.range_u64(lo, hi.max(lo + 1))
    }
    /// Modified time between `min_days` and `max_days` ago.
    fn age(&mut self, min_days: u64, max_days: u64) -> u64 {
        NOW - self.rng.range_u64(min_days, max_days.max(min_days + 1)) * DAY
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.rng.range_u64(0, items.len() as u64) as usize]
    }
}

fn file(name: &str, size: u64, modified: u64) -> FileNode {
    FileNode { name: name.to_string(), path: PathBuf::new(), size, is_dir: false, file_count: 0, modified, children: Vec::new() }
}

fn dir(name: &str, children: Vec<FileNode>) -> FileNode {
    FileNode { name: name.to_string(), path: PathBuf::new(), size: 0, is_dir: true, file_count: 0, modified: 0, children }
}

/// Fill in paths, directory sizes, file counts and newest-child dates.
fn finalize(node: &mut FileNode, parent: &std::path::Path) {
    node.path = parent.join(&node.name);
    if !node.is_dir {
        return;
    }
    let mut size = 0;
    let mut count = 0;
    let mut newest = 0;
    let path = node.path.clone();
    for c in &mut node.children {
        finalize(c, &path);
        size += c.size;
        count += if c.is_dir { c.file_count } else { 1 };
        newest = newest.max(c.modified);
    }
    node.size = size;
    node.file_count = count;
    node.modified = newest;
}

/// `n` files named `prefix_XXXX.ext` with sizes in [lo, hi] and ages in a band.
fn batch(g: &mut Gen, n: usize, prefix: &str, exts: &[&str], lo: u64, hi: u64, min_days: u64, max_days: u64) -> Vec<FileNode> {
    (0..n)
        .map(|i| {
            let ext = g.pick(exts);
            let size = g.size(lo, hi);
            let modified = g.age(min_days, max_days);
            file(&format!("{}_{:04}.{}", prefix, i + 1, ext), size, modified)
        })
        .collect()
}

fn video_project(g: &mut Gen, name: &str, clips: usize, days: u64) -> FileNode {
    let mut kids = batch(g, clips, "clip", &["mp4", "mov", "mkv"], 400 * MB, 6 * GB, days, days + 40);
    kids.push(file("timeline.prproj", g.size(20 * MB, 90 * MB), g.age(days, days + 5)));
    kids.push(file("final_export.mp4", g.size(5 * GB, 14 * GB), g.age(days, days + 3)));
    kids.push(dir("audio", batch(g, 14, "take", &["wav", "flac"], 40 * MB, 900 * MB, days, days + 30)));
    kids.push(dir("thumbnails", batch(g, 30, "thumb", &["png", "psd"], 2 * MB, 60 * MB, days, days + 20)));
    dir(name, kids)
}

fn unity_project(g: &mut Gen, name: &str, scale: u64, days: u64) -> FileNode {
    let assets = dir("Assets", vec![
        dir("Art", batch(g, 220, "tex", &["png", "psd", "tga"], 1 * MB, 90 * MB * scale, days, days + 400)),
        dir("Models", batch(g, 90, "mesh", &["fbx", "blend"], 4 * MB, 220 * MB * scale, days, days + 500)),
        dir("Audio", batch(g, 140, "sfx", &["wav", "ogg"], 200 * KB, 40 * MB, days, days + 600)),
        dir("Scripts", batch(g, 460, "Behaviour", &["cs"], 2 * KB, 90 * KB, days, days + 200)),
        dir("Scenes", batch(g, 18, "Level", &["unity"], 3 * MB, 60 * MB, days, days + 90)),
        dir("Prefabs", batch(g, 260, "Prefab", &["prefab"], 40 * KB, 6 * MB, days, days + 300)),
        dir("Plugins", batch(g, 34, "lib", &["dll", "so", "aar"], 300 * KB, 120 * MB, days + 100, days + 800)),
    ]);
    let library = dir("Library", vec![
        dir("Artifacts", batch(g, 900, "artifact", &["bin"], 200 * KB, 30 * MB * scale, days, days + 30)),
        dir("ShaderCache", batch(g, 420, "shader", &["bin"], 30 * KB, 4 * MB, days, days + 15)),
        dir("PackageCache", batch(g, 160, "pkg", &["cs", "dll", "json"], 20 * KB, 20 * MB, days, days + 120)),
        dir("Bee", batch(g, 120, "build", &["dag", "json"], 100 * KB, 40 * MB, days, days + 7)),
    ]);
    let builds = dir("Builds", vec![
        file(&format!("{}_Win64.zip", name), g.size(2 * GB, 6 * GB * scale), g.age(days, days + 30)),
        file(&format!("{}_Quest.apk", name), g.size(900 * MB, 3 * GB), g.age(days, days + 40)),
    ]);
    dir(name, vec![assets, library, builds, dir("Logs", batch(g, 40, "Editor", &["log"], 30 * KB, 20 * MB, days, days + 60)), dir("Temp", batch(g, 60, "tmp", &["tmp"], 10 * KB, 30 * MB, days, days + 3))])
}

fn node_modules(g: &mut Gen, n: usize, days: u64) -> FileNode {
    let names = ["react", "webpack", "typescript", "esbuild", "vite", "lodash", "three", "eslint", "prettier", "zod", "vitest", "tailwindcss", "chalk", "commander", "axios", "express", "rollup", "postcss", "sharp", "puppeteer"];
    let mut kids = Vec::new();
    for i in 0..n {
        let base = names[i % names.len()];
        let name = if i < names.len() { base.to_string() } else { format!("{}-{}", base, i / names.len()) };
        let files = g.rng.range_u64(8, 140) as usize;
        kids.push(dir(&name, batch(g, files, "index", &["js", "d.ts", "map", "json", "md"], 1 * KB, 3 * MB, days, days + 300)));
    }
    dir("node_modules", kids)
}

fn steam_game(g: &mut Gen, name: &str, size_gb: u64, days: u64) -> FileNode {
    let paks = (size_gb * 2).max(3) as usize;
    let mut kids = batch(g, paks, "pak", &["pak", "bin", "ucas", "utoc"], 200 * MB, 3 * GB, days, days + 700);
    kids.push(file(&format!("{}.exe", name.replace(' ', "")), g.size(40 * MB, 300 * MB), g.age(days, days + 30)));
    kids.push(dir("Content", batch(g, 60, "asset", &["uasset", "umap", "bnk"], 8 * MB, 400 * MB, days, days + 700)));
    kids.push(dir("Engine", batch(g, 80, "shader", &["dll", "ushaderbytecode", "bin"], 1 * MB, 90 * MB, days, days + 700)));
    dir(name, kids)
}

pub fn showcase_tree() -> FileNode {
    let mut g = Gen { rng: Rng::new(0x5EED_5EED) };
    let g = &mut g;

    let videos = dir("Videos", vec![
        video_project(g, "DUST NAP trailer", 26, 3),
        video_project(g, "Devlog 14 - the great refactor", 18, 41),
        video_project(g, "VR playtest night", 34, 120),
        dir("Captures", batch(g, 70, "OBS", &["mkv", "mp4"], 300 * MB, 4 * GB, 1, 400)),
    ]);
    let pictures = dir("Pictures", vec![
        dir("Screenshots", batch(g, 640, "Screenshot", &["png"], 800 * KB, 9 * MB, 0, 500)),
        dir("Camera Roll", batch(g, 1200, "IMG", &["jpg", "heic"], 2 * MB, 14 * MB, 30, 1400)),
        dir("Reference art", batch(g, 180, "ref", &["jpg", "png", "webp"], 300 * KB, 20 * MB, 60, 900)),
        dir("Raw", batch(g, 90, "DSC", &["cr2", "dng"], 22 * MB, 48 * MB, 200, 1200)),
    ]);
    let downloads = dir("Downloads", vec![
        file("Unity-6000.2.4f1-Setup.exe", g.size(3 * GB, 4 * GB), g.age(12, 20)),
        file("Blender-4.5-windows-x64.zip", g.size(300 * MB, 400 * MB), g.age(90, 120)),
        file("SyntyStudios_PolygonWestern.unitypackage", g.size(1 * GB, 2 * GB), g.age(40, 60)),
        file("ubuntu-24.04.iso", g.size(5 * GB, 6 * GB), g.age(300, 400)),
        file("cuda_12.8_windows.exe", g.size(3 * GB, 4 * GB), g.age(150, 200)),
        dir("Fonts", batch(g, 42, "font", &["ttf", "otf", "zip"], 100 * KB, 8 * MB, 10, 800)),
        dir("Old installers", batch(g, 28, "setup", &["exe", "msi"], 40 * MB, 900 * MB, 400, 1600)),
        dir("Sample packs", batch(g, 300, "kick", &["wav"], 400 * KB, 30 * MB, 100, 900)),
    ]);
    let documents = dir("Documents", vec![
        dir("Unity Projects", vec![
            unity_project(g, "DUST NAP", 2, 1),
            unity_project(g, "Monke Portals", 1, 9),
            unity_project(g, "EOS-Native Demo", 1, 25),
        ]),
        dir("Rust", vec![
            dir("spaceview", vec![dir("target", batch(g, 800, "dep", &["rlib", "rmeta", "o", "exe"], 100 * KB, 60 * MB, 0, 30)), dir("src", batch(g, 12, "mod", &["rs"], 4 * KB, 90 * KB, 0, 60))]),
            dir("tronteq", vec![dir("target", batch(g, 640, "dep", &["rlib", "rmeta", "o"], 100 * KB, 40 * MB, 20, 90)), dir("src", batch(g, 20, "mod", &["rs"], 4 * KB, 60 * KB, 20, 200))]),
        ]),
        dir("web", vec![
            dir("portfolio", vec![node_modules(g, 380, 5), dir("src", batch(g, 60, "page", &["html", "css", "js"], 4 * KB, 400 * KB, 1, 200))]),
            dir("deadends", vec![node_modules(g, 210, 40), dir("versions", batch(g, 12, "dead_ends_v", &["html"], 400 * KB, 700 * KB, 1, 40))]),
        ]),
        dir("Writing", batch(g, 120, "notes", &["md", "docx", "txt"], 2 * KB, 3 * MB, 1, 1500)),
        dir("Taxes", batch(g, 40, "form", &["pdf"], 100 * KB, 9 * MB, 200, 2000)),
    ]);
    let appdata = dir("AppData", vec![
        dir("Local", vec![
            dir("Google", vec![dir("Chrome", vec![dir("User Data", vec![dir("Default", vec![dir("Cache", batch(g, 2400, "f_", &["", "bin"], 4 * KB, 12 * MB, 0, 20)), dir("Code Cache", batch(g, 900, "chunk", &["bin"], 8 * KB, 4 * MB, 0, 20))])])])]),
            dir("Discord", batch(g, 300, "cache", &["bin", "png", "gif"], 20 * KB, 30 * MB, 0, 90)),
            dir("Temp", batch(g, 1400, "tmp", &["tmp", "log", "dmp"], 2 * KB, 400 * MB, 0, 60)),
            dir("Unity", vec![dir("cache", batch(g, 520, "pkg", &["tgz", "bin"], 200 * KB, 300 * MB, 10, 500))]),
            dir("pip", vec![dir("cache", batch(g, 640, "wheel", &["whl", "tar.gz"], 100 * KB, 800 * MB, 5, 600))]),
            dir("ollama", vec![dir("models", batch(g, 6, "llama3-blob", &["bin"], 2 * GB, 9 * GB, 5, 200))]),
        ]),
        dir("Roaming", vec![
            dir("Code", batch(g, 800, "ext", &["js", "json", "vsix"], 10 * KB, 40 * MB, 0, 300)),
            dir("obs-studio", batch(g, 30, "profile", &["json", "ini"], 1 * KB, 200 * KB, 5, 400)),
            dir("SpaceView", vec![file("prefs.ini", 2 * KB, g.age(0, 1))]),
        ]),
    ]);
    let trent = dir("trent", vec![videos, pictures, downloads, documents, appdata,
        dir("Desktop", batch(g, 26, "todo", &["txt", "png", "lnk", "zip"], 1 * KB, 900 * MB, 0, 200)),
        dir(".cargo", vec![dir("registry", batch(g, 1800, "crate", &["crate", "rs", "toml"], 20 * KB, 30 * MB, 0, 700))]),
    ]);
    let users = dir("Users", vec![trent, dir("Public", batch(g, 12, "shared", &["mp4", "png"], 5 * MB, 2 * GB, 300, 1800))]);

    let steam = dir("Steam", vec![dir("steamapps", vec![
        dir("common", vec![
            steam_game(g, "Project Demigod", 14, 3),
            steam_game(g, "Half-Life Alyx", 62, 500),
            steam_game(g, "Boneworks", 28, 900),
            steam_game(g, "Gorilla Tag", 3, 20),
            steam_game(g, "Deep Rock Galactic", 4, 60),
            steam_game(g, "Beat Saber", 2, 15),
            steam_game(g, "Blade and Sorcery", 21, 200),
            steam_game(g, "Elden Ring", 58, 700),
        ]),
        dir("shadercache", batch(g, 900, "shader", &["bin"], 100 * KB, 40 * MB, 0, 300)),
        dir("workshop", batch(g, 340, "item", &["vpk", "zip"], 2 * MB, 900 * MB, 10, 800)),
    ])]);
    let program_files = dir("Program Files", vec![
        dir("Unity", vec![dir("Hub", vec![dir("Editor", vec![
            dir("6000.2.4f1", batch(g, 2600, "unity", &["dll", "exe", "pdb", "bin"], 100 * KB, 300 * MB, 12, 20)),
            dir("2022.3.62f1", batch(g, 2400, "unity", &["dll", "exe", "pdb", "bin"], 100 * KB, 300 * MB, 200, 260)),
        ])])]),
        dir("Adobe", batch(g, 900, "adobe", &["dll", "exe", "dat"], 200 * KB, 400 * MB, 100, 400)),
        dir("Blender Foundation", batch(g, 700, "blender", &["dll", "py", "blend"], 40 * KB, 200 * MB, 90, 120)),
        dir("NVIDIA Corporation", batch(g, 400, "nv", &["dll", "exe", "bin"], 200 * KB, 600 * MB, 30, 60)),
        dir("JetBrains", batch(g, 1100, "rider", &["jar", "dll", "exe"], 40 * KB, 80 * MB, 20, 120)),
    ]);
    let windows = dir("Windows", vec![
        dir("System32", batch(g, 3400, "sys", &["dll", "exe", "sys", "mui"], 30 * KB, 40 * MB, 400, 2400)),
        dir("WinSxS", batch(g, 4200, "component", &["dll", "manifest", "cat"], 10 * KB, 20 * MB, 100, 2400)),
        dir("SoftwareDistribution", batch(g, 300, "update", &["cab", "psf", "esd"], 1 * MB, 900 * MB, 5, 200)),
        dir("Installer", batch(g, 220, "msi", &["msi", "msp"], 500 * KB, 400 * MB, 100, 1800)),
        dir("Temp", batch(g, 600, "tmp", &["tmp", "log"], 1 * KB, 60 * MB, 0, 40)),
    ]);
    let program_data = dir("ProgramData", vec![
        dir("Package Cache", batch(g, 260, "pkg", &["exe", "msi", "cab"], 1 * MB, 700 * MB, 100, 1500)),
        dir("Microsoft", batch(g, 800, "ms", &["dat", "log", "db"], 8 * KB, 200 * MB, 0, 900)),
        dir("NVIDIA", batch(g, 120, "cache", &["bin"], 1 * MB, 300 * MB, 5, 200)),
    ]);
    let mut root = dir("C:", vec![users, steam, program_files, windows, program_data,
        file("pagefile.sys", 16 * GB, g.age(0, 1)),
        file("hiberfil.sys", 26 * GB, g.age(0, 3)),
        dir("Github", vec![
            dir("EOS-Native", batch(g, 900, "src", &["cs", "meta", "dll", "png"], 2 * KB, 30 * MB, 0, 400)),
            dir("trentsterling.github.io", batch(g, 500, "page", &["html", "png", "css"], 3 * KB, 9 * MB, 0, 300)),
        ]),
    ]);
    // `Path::join` on "C:" yields "C:Users" (drive-relative), so children are
    // finalized against the real drive root and the root is summed by hand.
    let drive = PathBuf::from("C:\\");
    let mut size = 0;
    let mut count = 0;
    let mut newest = 0;
    for c in &mut root.children {
        finalize(c, &drive);
        size += c.size;
        count += if c.is_dir { c.file_count } else { 1 };
        newest = newest.max(c.modified);
    }
    root.path = drive;
    root.size = size;
    root.file_count = count;
    root.modified = newest;
    root
}
