#![windows_subsystem = "windows"]

mod app;
mod camera;
mod scanner;
mod stress;
mod treemap;
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
    let synthetic_n = stress::parse_flag_usize(&args, "--synthetic");
    let stress_s = stress::parse_flag_f32(&args, "--stress");

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("Failed to load icon");

    let prefs = app::load_prefs();

    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_title("SpaceView")
        .with_icon(std::sync::Arc::new(icon))
        .with_min_inner_size([400.0, 300.0]);

    // Restore saved window size, or default to 1024x700
    if let (Some(w), Some(h)) = (prefs.window_w, prefs.window_h) {
        vp = vp.with_inner_size([w, h]);
    } else {
        vp = vp.with_inner_size([1024.0, 700.0]);
    }

    // Restore saved window position (monitor placement)
    if let (Some(x), Some(y)) = (prefs.window_x, prefs.window_y) {
        vp = vp.with_position([x, y]);
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
            Ok(Box::new(app))
        }),
    )
}
