mod app;
mod camera;
mod scanner;
mod treemap;
mod world_layout;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SpaceView")
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
