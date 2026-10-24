mod app;
mod model;
mod scan;
mod trash;

use app::MacCleanApp;
use model::fmt_size;

fn main() -> eframe::Result<()> {
    scan::raise_fd_limit();

    // `macclean --dump` — headless scan, one line per target. Handy for
    // checking scanner output without opening the GUI.
    if std::env::args().any(|a| a == "--dump") {
        let r = scan::scan_all();
        for t in &r.targets {
            println!(
                "{:>8}  {:<7}  {:?}  {}",
                fmt_size(t.size),
                match t.safety {
                    model::Safety::Regenerable => "safe",
                    model::Safety::Review => "review",
                },
                t.action,
                t.path.display(),
            );
        }
        eprintln!("{} targets, {} errors", r.targets.len(), r.errors.len());
        return Ok(());
    }

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
