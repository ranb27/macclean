use crate::model::{fmt_size, Category, CleanTarget, Safety};
use crate::scan::{self, ScanResult};
use crate::trash;
use eframe::egui;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::Duration;

pub struct MacCleanApp {
    targets: Vec<CleanTarget>,
    scanning: bool,
    scan_rx: Option<Receiver<ScanResult>>,
    del_rx: Option<Receiver<(PathBuf, Result<(), String>)>>,
    deleting: bool,
    status: String,
    errors: Vec<String>,
    category_filter: Option<Category>,
}

impl Default for MacCleanApp {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            scanning: false,
            scan_rx: None,
            del_rx: None,
            deleting: false,
            status: "Press Scan to find reclaimable space".into(),
            errors: Vec::new(),
            category_filter: None,
        }
    }
}

impl MacCleanApp {
    fn start_scan(&mut self) {
        let (tx, rx) = channel();
        self.scan_rx = Some(rx);
        self.scanning = true;
        self.errors.clear();
        self.status = "Scanning…".into();
        std::thread::spawn(move || {
            let _ = tx.send(scan::scan_all());
        });
    }

    fn start_delete(&mut self) {
        let (tx, rx) = channel();
        self.del_rx = Some(rx);
        self.deleting = true;
        let selected: Vec<_> = self
            .targets
            .iter()
            .filter(|t| t.selected)
            .map(|t| (t.path.clone(), t.action))
            .collect();
        self.status = format!("Moving {} items to Trash…", selected.len());
        self.errors.clear();
        std::thread::spawn(move || {
            for (path, action) in selected {
                let result = trash::remove(&path, action);
                if tx.send((path, result)).is_err() {
                    break;
                }
            }
        });
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan_rx else { return };
        match rx.try_recv() {
            Ok(result) => {
                let total: u64 = result.targets.iter().map(|t| t.size).sum();
                self.status = format!(
                    "Found {} targets — {} potentially reclaimable",
                    result.targets.len(),
                    fmt_size(total)
                );
                self.errors = result.errors;
                self.targets = result.targets;
                self.scanning = false;
                self.scan_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.scanning = false;
                self.scan_rx = None;
                self.status = "Scan failed".into();
            }
        }
    }

    fn poll_delete(&mut self) {
        let mut disconnected = false;
        let mut done_paths = HashSet::new();
        if let Some(rx) = &self.del_rx {
            loop {
                match rx.try_recv() {
                    Ok((path, Ok(()))) => {
                        done_paths.insert(path);
                    }
                    Ok((path, Err(e))) => {
                        self.errors.push(format!("{}: {}", path.display(), e));
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if !done_paths.is_empty() {
            let freed: u64 = self
                .targets
                .iter()
                .filter(|t| done_paths.contains(&t.path))
                .map(|t| t.size)
                .sum();
            self.targets.retain(|t| !done_paths.contains(&t.path));
            self.status = format!("Moved items to Trash — {} freed", fmt_size(freed));
        }
        if disconnected {
            self.deleting = false;
            self.del_rx = None;
        }
    }

    fn selected_total(&self) -> (usize, u64) {
        self.targets
            .iter()
            .filter(|t| t.selected)
            .fold((0, 0), |(n, s), t| (n + 1, s + t.size))
    }
}

impl eframe::App for MacCleanApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_scan();
        self.poll_delete();
        if self.scanning || self.deleting {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("macclean");
                ui.separator();
                let busy = self.scanning || self.deleting;
                if ui.add_enabled(!busy, egui::Button::new("Scan")).clicked() {
                    self.start_scan();
                }
                ui.separator();
                ui.label(&self.status);
            });
        });

        egui::Panel::left("cats").show(ui, |ui| {
            ui.heading("Categories");
            ui.add_space(6.0);
            for cat in Category::ALL {
                let (count, size) = self
                    .targets
                    .iter()
                    .filter(|t| t.category == cat)
                    .fold((0usize, 0u64), |(n, s), t| (n + 1, s + t.size));
                let selected = self.category_filter == Some(cat);
                if ui
                    .selectable_label(
                        selected,
                        format!("{}  ({} · {})", cat.label(), count, fmt_size(size)),
                    )
                    .clicked()
                {
                    self.category_filter = if selected { None } else { Some(cat) };
                }
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label("Select:");
            ui.horizontal(|ui| {
                if ui.button("safe").clicked() {
                    for t in &mut self.targets {
                        t.selected = matches!(t.safety, Safety::Regenerable);
                    }
                }
                if ui.button("all").clicked() {
                    for t in &mut self.targets {
                        t.selected = true;
                    }
                }
                if ui.button("none").clicked() {
                    for t in &mut self.targets {
                        t.selected = false;
                    }
                }
            });
        });

        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.horizontal(|ui| {
                let (n, size) = self.selected_total();
                let label = format!("Move {} items to Trash ({})", n, fmt_size(size));
                let btn = ui.add_enabled(
                    n > 0 && !self.deleting && !self.scanning,
                    egui::Button::new(label),
                );
                if btn.clicked() {
                    self.start_delete();
                }
                if self.deleting {
                    ui.spinner();
                }
            });
            if !self.errors.is_empty() {
                ui.separator();
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!("{} error(s) — hover for details", self.errors.len()),
                )
                .on_hover_text(self.errors.join("\n"));
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.scanning {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
                return;
            }
            if self.targets.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Nothing found — press Scan");
                });
                return;
            }

            egui::Grid::new("header").num_columns(5).show(ui, |ui| {
                ui.label("");
                ui.strong("Size");
                ui.strong("Safety");
                ui.strong("Path");
                ui.strong("Reason");
                ui.end_row();
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    egui::Grid::new("rows").num_columns(5).striped(true).show(ui, |ui| {
                        let filter = self.category_filter;
                        for t in self
                            .targets
                            .iter_mut()
                            .filter(|t| filter.map_or(true, |c| t.category == c))
                        {
                            ui.checkbox(&mut t.selected, "");
                            ui.monospace(fmt_size(t.size));
                            match t.safety {
                                Safety::Regenerable => {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(0x3c, 0xc9, 0x7e),
                                        "safe",
                                    );
                                }
                                Safety::Review => {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(0xe6, 0xa2, 0x3c),
                                        "review",
                                    );
                                }
                            }
                            let path_str = t.path.display().to_string();
                            ui.label(&path_str).on_hover_text(path_str);
                            ui.label(&t.reason);
                            ui.end_row();
                        }
                    });
                });
        });
    }
}
