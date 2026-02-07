mod app;
mod camera;
mod scanner;
mod treemap;
mod world_layout;

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("Failed to load icon");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SpaceView")
            .with_icon(std::sync::Arc::new(icon))
            .with_inner_size([1024.0, 700.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SpaceView",
        options,
        Box::new(|cc| Ok(Box::new(app::SpaceViewApp::new(cc)))),
    )
}
