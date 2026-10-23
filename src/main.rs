mod app;
mod model;
mod scan;
mod trash;

use app::MacCleanApp;

fn main() -> eframe::Result<()> {
    scan::raise_fd_limit();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title("macclean"),
        ..Default::default()
    };
    eframe::run_native(
        "macclean",
        options,
        Box::new(|_cc| Ok(Box::new(MacCleanApp::default()))),
    )
}
