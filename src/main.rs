#![windows_subsystem = "windows"]

mod app;
mod camera;
mod color;
mod contrast;
mod gauntlet;
mod scanner;
mod scan_cache;
mod journal;
mod anonymize;
mod shots;
mod showcase;
mod stress;
mod theme;
mod treemap;
mod window_chrome;
mod world_layout;

/// Write panics to %APPDATA%/SpaceView/panic.log. The app runs with
/// windows_subsystem = "windows", so stderr is lost; without this, crashes
/// in the field are undiagnosable.
fn install_panic_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = std::path::PathBuf::from(appdata).join("SpaceView");
            let _ = std::fs::create_dir_all(&dir);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let msg = format!(
                "[unix {}] SpaceView {} panic: {}\nbacktrace:\n{}\n",
                ts,
                env!("CARGO_PKG_VERSION"),
                info,
                std::backtrace::Backtrace::force_capture()
            );
            let _ = std::fs::write(dir.join("panic.log"), msg);
        }
        default(info);
    }));
}

fn main() -> eframe::Result<()> {
    install_panic_log();

    // Permanent perf harness: --synthetic N (in-memory fake tree, no scan)
    // and --stress S (scripted camera thrash for S seconds, CSV metrics, exit).
    let args: Vec<String> = std::env::args().collect();
    if let Some(dir) = shots::parse_flag_path(&args, "--cache-gauntlet") {
        scan_cache::gauntlet(&dir);
        return Ok(());
    }
    if let Some(path) = shots::parse_flag_path(&args, "--cache-probe") {
        let dir = shots::parse_flag_path(&args, "--report-dir")
            .unwrap_or_else(|| std::path::PathBuf::from("target/gauntlet/cache-probe"));
        scan_cache::probe(&path, &dir, args.iter().any(|arg| arg == "--force-full"),
            args.iter().any(|arg| arg == "--expect-reuse"));
        return Ok(());
    }
    if let Some(path) = shots::parse_flag_path(&args, "--scan-probe") {
        let dir = shots::parse_flag_path(&args, "--report-dir")
            .unwrap_or_else(|| std::path::PathBuf::from("target/gauntlet/probe"));
        let seconds = stress::parse_flag_f32(&args, "--probe-seconds").unwrap_or(5.0);
        gauntlet::scan_probe(&path, &dir, seconds,
            !args.iter().any(|arg| arg == "--probe-without-cache"),
            !args.iter().any(|arg| arg == "--probe-without-previews"),
            stress::parse_flag_usize(&args, "--probe-files").unwrap_or(0) as u64);
        return Ok(());
    }
    let synthetic_n = stress::parse_flag_usize(&args, "--synthetic");
    let stress_s = stress::parse_flag_f32(&args, "--stress");
    // Scripted screenshots: --shots DIR (with --synthetic N), see shots.rs.
    let shots_dir = shots::parse_flag_path(&args, "--shots");
    // --scan PATH with --shots: real scan, anonymized in memory before drawing (anonymize.rs).
    let shots_scan = shots::parse_flag_path(&args, "--scan");
    let live_shots_dir = shots::parse_flag_path(&args, "--live-shots");
    let cache_shots_dir = shots::parse_flag_path(&args, "--cache-shots");

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("Failed to load icon");

    let prefs = app::load_prefs();

    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_title("SpaceView")
        .with_icon(std::sync::Arc::new(icon))
        .with_min_inner_size([400.0, 300.0])
        // Frameless: the top toolbar IS the title bar. See window_chrome.rs.
        .with_decorations(false);

    // Restore saved window size, or default to 1024x700
    if shots_dir.is_some() || live_shots_dir.is_some() || cache_shots_dir.is_some() {
        // Fixed frame for the landing page shots, regardless of saved prefs.
        let width = stress::parse_flag_f32(&args, "--shot-width").unwrap_or(1400.0);
        let height = stress::parse_flag_f32(&args, "--shot-height").unwrap_or(860.0);
        vp = vp.with_inner_size([width, height]).with_position([40.0, 40.0]);
    } else if let (Some(w), Some(h)) = (prefs.window_w, prefs.window_h) {
        vp = vp.with_inner_size([w, h]);
    } else {
        vp = vp.with_inner_size([1024.0, 700.0]);
    }

    // Restore saved window position (monitor placement)
    if shots_dir.is_none() && live_shots_dir.is_none() && cache_shots_dir.is_none() {
        if let (Some(x), Some(y)) = (prefs.window_x, prefs.window_y) {
            vp = vp.with_position([x, y]);
        }
    }

    let options = eframe::NativeOptions {
        viewport: vp,
        ..Default::default()
    };

    eframe::run_native(
        "SpaceView",
        options,
        Box::new(move |cc| {
            let mut app = app::SpaceViewApp::new(cc);
            app.configure_stress(synthetic_n, stress_s);
            if let Some(dir) = shots_dir.clone() {
                app.configure_shots(dir, shots_scan.clone());
            } else if let Some(dir) = live_shots_dir.clone() {
                app.configure_live_shots(dir, shots_scan.clone().expect("--live-shots needs --scan PATH"));
            } else if let Some(dir) = cache_shots_dir.clone() {
                app.configure_cache_shots(dir, shots_scan.clone().expect("--cache-shots needs --scan PATH"));
            } else if let Some(path) = shots_scan.clone() {
                app.open_scan(path);
            }
            Ok(Box::new(app))
        }),
    )
}
