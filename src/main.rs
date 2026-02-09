#![windows_subsystem = "windows"]

mod app;
mod camera;
mod scanner;
mod treemap;
mod world_layout;

fn main() -> eframe::Result<()> {
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
        Box::new(|cc| Ok(Box::new(app::SpaceViewApp::new(cc)))),
    )
}
