mod app;
mod config;
mod connect;
mod mcp;
mod model;
mod scan;
mod schedule;
mod trash;
mod tray;

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

    // `macclean --mcp` — MCP server on stdio. Lets the user's AI client
    // scan/clean via tools; clean is gated to regenerable targets only.
    if std::env::args().any(|a| a == "--mcp") {
        if let Err(e) = mcp::run() {
            eprintln!("mcp server: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // `macclean --scheduled-scan` — what the LaunchAgent runs: scans,
    // records the summary, posts a notification. Never deletes.
    if std::env::args().any(|a| a == "--scheduled-scan") {
        match schedule::run_scheduled() {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("scheduled scan: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // `macclean --schedule daily|weekly|off` — install/remove the
    // LaunchAgent from the CLI, same as the Schedule window does in-app.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--schedule") {
        let freq = match args.get(i + 1).map(String::as_str) {
            Some("daily") => Some(schedule::Frequency::Daily),
            Some("weekly") => Some(schedule::Frequency::Weekly),
            Some("off") => None,
            _ => {
                eprintln!("usage: macclean --schedule daily|weekly|off");
                std::process::exit(2);
            }
        };
        match schedule::set(freq) {
            Ok(m) => println!("{m}"),
            Err(e) => {
                eprintln!("schedule: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // `--tray` keeps a menu-bar icon resident; closing the window then
    // hides it instead of quitting — Quit lives on the tray menu.
    let tray_mode = args.iter().any(|a| a == "--tray");

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-1024.png")).ok();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1160.0, 720.0])
        .with_min_inner_size([940.0, 560.0])
        .with_title("macclean");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "macclean",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            app::apply_theme(&cc.egui_ctx);
            app::load_fonts(&cc.egui_ctx);
            Ok(Box::new(MacCleanApp::new(tray_mode)))
        }),
    )
}
