use crate::connect;
use crate::model::{Action, Category, CleanTarget, Safety, fmt_size};
use crate::scan::{self, ScanResult};
use crate::trash;
use eframe::egui;
use egui::{
    Align, Align2, Color32, CornerRadius, FontData, FontFamily, FontId, Layout, Margin, Pos2,
    RichText, Stroke, StrokeKind, TextStyle, vec2,
};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

// ---------- palette (matches app icon + landing page) ----------

/// Per-theme colors — the app follows macOS light/dark automatically.
struct Palette {
    bg: Color32,
    grad_top: Color32,
    grad_bot: Color32,
    text: Color32,
    muted: Color32,
    green: Color32,
    teal: Color32,
    amber: Color32,
    red: Color32,
    btn_text: Color32,
    card: Color32,
    card_stroke: Color32,
    hover: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0d, 0x11, 0x17),
    grad_top: Color32::from_rgb(0x13, 0x1a, 0x24),
    grad_bot: Color32::from_rgb(0x0a, 0x0e, 0x14),
    text: Color32::from_rgb(0xdb, 0xe4, 0xef),
    muted: Color32::from_rgb(0x85, 0x91, 0xa2),
    green: Color32::from_rgb(0x3c, 0xc9, 0x7e),
    teal: Color32::from_rgb(0x38, 0xbd, 0xf8),
    amber: Color32::from_rgb(0xe6, 0xa2, 0x3c),
    red: Color32::from_rgb(0xef, 0x5f, 0x6b),
    btn_text: Color32::from_rgb(0x0a, 0x14, 0x0e),
    card: Color32::from_rgba_unmultiplied_const(255, 255, 255, 14),
    card_stroke: Color32::from_rgba_unmultiplied_const(255, 255, 255, 30),
    hover: Color32::from_rgba_unmultiplied_const(255, 255, 255, 32),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf4, 0xf6, 0xf9),
    grad_top: Color32::from_rgb(0xf0, 0xf3, 0xf8),
    grad_bot: Color32::from_rgb(0xe8, 0xed, 0xf3),
    text: Color32::from_rgb(0x1a, 0x22, 0x2e),
    muted: Color32::from_rgb(0x64, 0x70, 0x82),
    green: Color32::from_rgb(0x1f, 0xa3, 0x5f),
    teal: Color32::from_rgb(0x0d, 0x8f, 0xcc),
    amber: Color32::from_rgb(0xa8, 0x77, 0x14),
    red: Color32::from_rgb(0xd8, 0x47, 0x54),
    btn_text: Color32::from_rgb(0xff, 0xff, 0xff),
    card: Color32::from_rgba_unmultiplied_const(255, 255, 255, 200),
    card_stroke: Color32::from_rgba_unmultiplied_const(0, 0, 0, 20),
    hover: Color32::from_rgba_unmultiplied_const(0, 0, 0, 10),
};

fn pal(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

fn bold() -> FontFamily {
    FontFamily::Name("inter-bold".into())
}

/// Theme: follow macOS light/dark. egui has no real backdrop blur, so the
/// "glass" feel comes from translucent cards over a soft gradient.
fn copy_clipboard(text: &str) {
    use std::io::Write;
    if let Ok(mut child) = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        && let Some(mut w) = child.stdin.take()
    {
        let _ = w.write_all(text.as_bytes());
        drop(w);
        let _ = child.wait();
    }
}
pub fn apply_theme(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            let p = if theme == egui::Theme::Dark {
                &DARK
            } else {
                &LIGHT
            };
            let mut v = if theme == egui::Theme::Dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };

            v.panel_fill = p.bg;
            v.window_fill = p.bg;
            v.extreme_bg_color = p.grad_bot;
            v.faint_bg_color = p.hover.gamma_multiply(0.4);
            v.hyperlink_color = p.teal;
            v.override_text_color = Some(p.text);
            v.window_corner_radius = CornerRadius::same(12);

            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = CornerRadius::same(8);
            }
            v.widgets.inactive.weak_bg_fill = p.card;
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.card_stroke);
            v.widgets.hovered.weak_bg_fill = p.hover;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.card_stroke.gamma_multiply(1.8));
            v.widgets.active.weak_bg_fill = p.hover.gamma_multiply(1.5);

            v.selection.bg_fill = p.green;
            v.selection.stroke = Stroke::new(1.0, p.green);

            style.visuals = v;
            style.spacing.item_spacing = vec2(10.0, 8.0);
            style.spacing.button_padding = vec2(14.0, 7.0);
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::proportional(20.0));
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(14.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(13.0));
            style
                .text_styles
                .insert(TextStyle::Monospace, FontId::monospace(12.0));
        });
    }
    ctx.set_theme(egui::ThemePreference::System);
}

/// Inter (embedded, SIL OFL) for text; "inter-bold" is a custom family for
/// headings/strong text since egui has no font-weight selection. SF Mono
/// for paths/sizes, falling back to egui's default mono.
pub fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "inter".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-sb".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "inter".into());
    fonts.families.insert(
        FontFamily::Name("inter-bold".into()),
        vec!["inter-sb".into(), "inter".into()],
    );

    if let Ok(data) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
        fonts
            .font_data
            .insert("sfmono".into(), FontData::from_owned(data).into());
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "sfmono".into());
    }
    ctx.set_fonts(fonts);
}

// ---------- update check ----------

const LATEST_API: &str = "https://api.github.com/repos/ranb27/macclean/releases/latest";
const DMG_URL: &str =
    "https://github.com/ranb27/macclean/releases/latest/download/macclean-macos-arm64.dmg";

#[derive(Clone)]
enum UpdateState {
    Idle,
    Checking,
    /// Some(tag) when a newer release exists.
    Done(Option<String>),
}

/// Latest release tag from GitHub, if it's newer than this binary.
/// curl ships with macOS — no HTTP dependency needed.
fn fetch_newer_release() -> Option<String> {
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "10", LATEST_API])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let tag = json_string_field(&body, "tag_name")?;
    version_newer(&tag, env!("CARGO_PKG_VERSION")).then_some(tag)
}

/// Minimal `"key": "value"` extraction — the response is only ever used
/// for tag_name, so a full JSON parser isn't worth a dependency.
fn json_string_field(body: &str, key: &str) -> Option<String> {
    let i = body.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = body[i..].split_once(':')?.1.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Lexicographic numeric compare — "v0.10.0" > "0.2.0".
fn version_newer(tag: &str, cur: &str) -> bool {
    fn parts(s: &str) -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|x| x.parse().ok())
            .collect()
    }
    parts(tag) > parts(cur)
}

pub struct MacCleanApp {
    targets: Vec<CleanTarget>,
    scanning: bool,
    scan_rx: Option<Receiver<ScanResult>>,
    del_rx: Option<Receiver<(PathBuf, Result<(), String>)>>,
    deleting: bool,
    status: String,
    errors: Vec<String>,
    category_filter: Option<Category>,
    update_state: UpdateState,
    update_rx: Option<Receiver<Option<String>>>,
    update_started: bool,
    show_connect: bool,
    connect_status: HashMap<&'static str, Result<String, String>>,
    test_result: Option<Result<String, String>>,
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
            update_state: UpdateState::Idle,
            update_rx: None,
            update_started: false,
            show_connect: false,
            connect_status: HashMap::new(),
            test_result: None,
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
        self.status = format!("Cleaning {} items…", selected.len());
        self.errors.clear();
        std::thread::spawn(move || {
            for (path, action) in selected {
                let result = trash::execute(&path, action);
                if tx.send((path, result)).is_err() {
                    break;
                }
            }
        });
    }

    /// Background check for a newer release — runs curl off the UI thread
    /// and repaints when it lands.
    fn check_updates(&mut self, ctx: &egui::Context) {
        if matches!(self.update_state, UpdateState::Checking) {
            return;
        }
        let (tx, rx) = channel();
        self.update_rx = Some(rx);
        self.update_state = UpdateState::Checking;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(fetch_newer_release());
            ctx.request_repaint();
        });
    }

    fn poll_update(&mut self) {
        let Some(rx) = &self.update_rx else { return };
        match rx.try_recv() {
            Ok(tag) => {
                self.update_state = UpdateState::Done(tag);
                self.update_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.update_state = UpdateState::Done(None);
                self.update_rx = None;
            }
        }
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
            self.status = format!("Cleaned — {} freed", fmt_size(freed));
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

    fn safety_color(p: &Palette, safety: Safety) -> Color32 {
        match safety {
            Safety::Regenerable => p.green,
            Safety::Review => p.amber,
        }
    }

    /// Rounded pill used for the safety column.
    fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
        let (rect, _) = ui.allocate_exact_size(vec2(58.0, 18.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), color.gamma_multiply(0.14));
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(9),
            Stroke::new(1.0, color.gamma_multiply(0.45)),
            StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(10.5),
            color,
        );
    }

    /// Size column: monospace number over a proportional bar.
    fn size_cell(ui: &mut egui::Ui, p: &Palette, size: u64, max: u64, safety: Safety) {
        ui.vertical(|ui| {
            if size == 0 {
                ui.label(RichText::new("—").monospace().color(p.muted));
            } else {
                ui.label(
                    RichText::new(fmt_size(size))
                        .monospace()
                        .size(12.0)
                        .color(p.text),
                );
            }
            let w = ui.available_width().min(110.0);
            let (rect, _) = ui.allocate_exact_size(vec2(w, 3.5), egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(2),
                Color32::BLACK.gamma_multiply(if ui.visuals().dark_mode { 0.3 } else { 0.08 }),
            );
            if size > 0 && max > 0 {
                let mut fill = rect;
                fill.max.x = rect.min.x + rect.width() * (size as f32 / max as f32);
                ui.painter().rect_filled(
                    fill,
                    CornerRadius::same(2),
                    Self::safety_color(p, safety).gamma_multiply(0.85),
                );
            }
        });
    }

    /// Reclaimable ring — fraction filled = share of total currently ticked.
    fn reclaim_ring(ui: &mut egui::Ui, p: &Palette, frac: f32, center_text: &str) {
        let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), egui::Sense::hover());
        let center = rect.center();
        let r = 24.0;
        ui.painter().circle_stroke(
            center,
            r,
            Stroke::new(6.0, p.card_stroke.gamma_multiply(1.4)),
        );
        if frac > 0.0 {
            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2
                        + frac * std::f32::consts::TAU * i as f32 / 40.0;
                    center + r * vec2(a.cos(), a.sin())
                })
                .collect();
            ui.painter().add(egui::epaint::PathShape::line(
                pts,
                egui::epaint::PathStroke::new(6.0, p.green),
            ));
        }
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            center_text,
            FontId::proportional(11.0),
            p.text,
        );
    }

    /// "Connect AI" window — one click per detected MCP client.
    fn connect_window(&mut self, ctx: &egui::Context, p: &Palette) {
        if !self.show_connect {
            return;
        }
        let mut open = true;
        egui::Window::new("Connect macclean to your AI")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Runs `macclean --mcp` as an MCP server — pick your assistant:")
                        .size(12.0)
                        .color(p.muted),
                );
                ui.add_space(8.0);
                for c in connect::CLIENTS {
                    let detected = connect::is_detected(c);
                    let wired = connect::is_wired(c);
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            vec2(130.0, 22.0),
                            egui::Label::new(RichText::new(c.name).size(13.0).color(p.text)),
                        );
                        if wired {
                            ui.label(RichText::new("connected").size(10.5).color(p.green));
                        } else if detected {
                            ui.label(RichText::new("installed").size(10.5).color(p.teal));
                        } else if !matches!(c.method, connect::Method::Manual) {
                            ui.label(RichText::new("not found").size(10.5).color(p.muted));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let manual = matches!(c.method, connect::Method::Manual);
                            let (label, enabled, color) = if manual {
                                ("Copy", true, p.text)
                            } else if wired {
                                ("Disconnect", true, p.red)
                            } else {
                                ("Connect", detected, if detected { p.text } else { p.muted })
                            };
                            let btn = ui.add_enabled(
                                enabled,
                                egui::Button::new(RichText::new(label).size(11.5).color(color)),
                            );
                            if btn.clicked() {
                                if manual {
                                    copy_clipboard(&connect::snippet());
                                    self.connect_status
                                        .insert(c.name, Ok("snippet copied".into()));
                                } else if wired {
                                    self.connect_status.insert(c.name, connect::disconnect(c));
                                } else {
                                    self.connect_status.insert(c.name, connect::connect(c));
                                }
                            }
                        });
                    });
                    match self.connect_status.get(c.name) {
                        Some(Ok(msg)) => {
                            ui.label(
                                RichText::new(format!("  {msg} — {}", c.note))
                                    .size(11.0)
                                    .color(p.green),
                            );
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(format!("  {e}")).size(11.0).color(p.red));
                        }
                        None => {}
                    }
                    ui.add_space(2.0);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            RichText::new("Test connection").size(11.5).color(p.text),
                        ))
                        .on_hover_text("Spawn `macclean --mcp` and run the MCP handshake")
                        .clicked()
                    {
                        self.test_result = Some(connect::test_server());
                    }
                    match &self.test_result {
                        Some(Ok(msg)) => {
                            ui.label(RichText::new(msg).size(11.0).color(p.green));
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).size(11.0).color(p.red));
                        }
                        None => {}
                    }
                });
                ui.label(
                    RichText::new(format!("Config snippet: {}", connect::snippet()))
                        .size(10.5)
                        .monospace()
                        .color(p.muted),
                );
                ui.label(
                    RichText::new(
                        "Ollama isn't an MCP client — use it through a front-end like Open WebUI.",
                    )
                    .size(10.5)
                    .color(p.muted),
                );
            });
        self.show_connect &= open;
    }

    /// Subtle vertical gradient behind the central content area.
    fn paint_gradient(ui: &mut egui::Ui, p: &Palette) {
        let rect = ui.max_rect();
        let uv = egui::epaint::WHITE_UV;
        let mesh = egui::epaint::Mesh {
            vertices: vec![
                egui::epaint::Vertex {
                    pos: rect.left_top(),
                    uv,
                    color: p.grad_top,
                },
                egui::epaint::Vertex {
                    pos: rect.right_top(),
                    uv,
                    color: p.grad_top,
                },
                egui::epaint::Vertex {
                    pos: rect.right_bottom(),
                    uv,
                    color: p.grad_bot,
                },
                egui::epaint::Vertex {
                    pos: rect.left_bottom(),
                    uv,
                    color: p.grad_bot,
                },
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..Default::default()
        };
        ui.painter().add(egui::Shape::mesh(mesh));
    }
}

impl eframe::App for MacCleanApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_scan();
        self.poll_delete();
        self.poll_update();
        if !self.update_started {
            self.update_started = true;
            self.check_updates(ui.ctx());
        }
        if self.scanning || self.deleting {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
        let busy = self.scanning || self.deleting;
        let p = pal(ui);

        // ---------- top toolbar ----------
        egui::Panel::top("top")
            .frame(egui::Frame::default().fill(p.bg).inner_margin(Margin {
                left: 16,
                right: 16,
                top: 14,
                bottom: 12,
            }))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/icon-1024.png"))
                            .fit_to_exact_size(vec2(26.0, 26.0)),
                    );
                    ui.label(RichText::new("macclean").size(18.0).family(bold()));
                    ui.add_space(10.0);

                    let label = if self.scanning { "Scanning…" } else { "Scan" };
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(
                                RichText::new(label).family(bold()).color(p.btn_text),
                            )
                            .fill(p.green)
                            .min_size(vec2(76.0, 30.0)),
                        )
                        .clicked()
                    {
                        self.start_scan();
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.status).color(p.muted).size(12.5));
                    if self.deleting {
                        ui.add(egui::Spinner::new().size(16.0).color(p.green));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                .color(p.muted)
                                .size(12.0),
                        );
                        ui.add_space(6.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Connect AI").size(11.0).color(p.teal),
                                )
                                .fill(p.card),
                            )
                            .on_hover_text("Wire macclean --mcp into your AI assistant")
                            .clicked()
                        {
                            self.show_connect = true;
                        }
                        ui.add_space(4.0);
                        match &self.update_state {
                            UpdateState::Done(Some(v)) => {
                                let btn = ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("Update {v}"))
                                                .family(bold())
                                                .size(11.5)
                                                .color(p.btn_text),
                                        )
                                        .fill(p.green),
                                    )
                                    .on_hover_text("Download the latest .dmg");
                                if btn.clicked() {
                                    let _ = std::process::Command::new("open").arg(DMG_URL).spawn();
                                }
                            }
                            UpdateState::Checking => {
                                ui.add(egui::Spinner::new().size(14.0).color(p.green));
                            }
                            UpdateState::Done(None) => {
                                ui.label(RichText::new("up to date").color(p.muted).size(11.0));
                            }
                            UpdateState::Idle => {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("check for updates")
                                                .size(11.0)
                                                .color(p.muted),
                                        )
                                        .fill(p.card),
                                    )
                                    .clicked()
                                {
                                    self.check_updates(ui.ctx());
                                }
                            }
                        }
                    });
                });
            });

        // ---------- bottom action bar ----------
        egui::Panel::bottom("bottom")
            .frame(egui::Frame::default().fill(p.bg).inner_margin(Margin {
                left: 16,
                right: 16,
                top: 12,
                bottom: 14,
            }))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (n, size) = self.selected_total();
                    ui.label(
                        RichText::new(format!("{n} selected · {}", fmt_size(size)))
                            .color(p.muted)
                            .size(12.5),
                    );
                    if !self.errors.is_empty() {
                        ui.add_space(8.0);
                        ui.colored_label(p.red, format!("{} error(s)", self.errors.len()))
                            .on_hover_text(self.errors.join("\n"));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn = ui.add_enabled(
                            n > 0 && !busy,
                            egui::Button::new(
                                RichText::new(format!("Clean {n} items · {}", fmt_size(size)))
                                    .family(bold())
                                    .color(p.btn_text)
                                    .size(14.0),
                            )
                            .fill(p.green)
                            .min_size(vec2(0.0, 34.0)),
                        );
                        if btn.clicked() {
                            self.start_delete();
                        }
                    });
                });
            });

        // ---------- left categories ----------
        egui::Panel::left("cats")
            .frame(egui::Frame::default().fill(p.bg).inner_margin(Margin {
                left: 16,
                right: 10,
                top: 6,
                bottom: 12,
            }))
            .exact_size(198.0)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("CATEGORIES")
                        .size(10.0)
                        .color(p.muted)
                        .strong(),
                );
                ui.add_space(6.0);

                for cat in Category::ALL {
                    let (count, size) = self
                        .targets
                        .iter()
                        .filter(|t| t.category == cat)
                        .fold((0usize, 0u64), |(n, s), t| (n + 1, s + t.size));
                    let selected = self.category_filter == Some(cat);

                    let (rect, resp) = ui.allocate_exact_size(
                        vec2(ui.available_width(), 34.0),
                        egui::Sense::click(),
                    );
                    let fill = if selected {
                        p.green.gamma_multiply(0.13)
                    } else if resp.hovered() {
                        p.hover
                    } else {
                        p.card
                    };
                    let stroke = if selected {
                        p.green.gamma_multiply(0.5)
                    } else {
                        p.card_stroke
                    };
                    ui.painter().rect(
                        rect,
                        CornerRadius::same(8),
                        fill,
                        Stroke::new(1.0, stroke),
                        StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(10.0, -7.0),
                        Align2::LEFT_CENTER,
                        cat.label(),
                        FontId::proportional(12.5),
                        if selected { p.green } else { p.text },
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(10.0, 8.0),
                        Align2::LEFT_CENTER,
                        format!("{count} items"),
                        FontId::proportional(10.5),
                        p.muted,
                    );
                    ui.painter().text(
                        rect.right_center() + vec2(-10.0, 0.0),
                        Align2::RIGHT_CENTER,
                        fmt_size(size),
                        FontId::monospace(11.0),
                        if selected { p.green } else { p.muted },
                    );
                    if resp.clicked() {
                        self.category_filter = if selected { None } else { Some(cat) };
                    }
                    ui.add_space(4.0);
                }

                // reclaimable ring + select shortcuts
                if !self.targets.is_empty() {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);
                    let total: u64 = self.targets.iter().map(|t| t.size).sum();
                    let (sel_n, sel_size) = self.selected_total();
                    let frac = if total > 0 {
                        sel_size as f32 / total as f32
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        Self::reclaim_ring(ui, p, frac, &fmt_size(total));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("reclaimable").size(11.0).color(p.muted));
                            ui.label(
                                RichText::new(format!("{sel_n} ticked"))
                                    .size(11.5)
                                    .color(p.text),
                            );
                        });
                    });
                }

                ui.add_space(10.0);
                ui.label(RichText::new("SELECT").size(10.0).color(p.muted).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let btns = [("safe", 0u8), ("all", 1u8), ("none", 2u8)];
                    for (label, mode) in btns {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label).size(11.5).color(if mode == 0 {
                                        p.green
                                    } else {
                                        p.muted
                                    }),
                                )
                                .min_size(vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            for t in &mut self.targets {
                                t.selected = match mode {
                                    0 => matches!(t.safety, Safety::Regenerable),
                                    1 => true,
                                    _ => false,
                                };
                            }
                        }
                    }
                });
            });

        // ---------- connect-AI window ----------
        self.connect_window(ui.ctx(), p);

        // ---------- central target list ----------
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(p.bg))
            .show(ui, |ui| {
                Self::paint_gradient(ui, p);

                if self.scanning {
                    let center = ui.max_rect().center();
                    let rect = egui::Rect::from_center_size(center, vec2(240.0, 90.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add(egui::Spinner::new().size(38.0).color(p.green));
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("Scanning your Mac…")
                                    .color(p.muted)
                                    .size(13.5),
                            );
                        });
                    });
                    return;
                }
                if self.targets.is_empty() {
                    let center = ui.max_rect().center();
                    let rect = egui::Rect::from_center_size(center, vec2(280.0, 160.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add(
                                egui::Image::new(egui::include_image!("../assets/icon-1024.png"))
                                    .fit_to_exact_size(vec2(64.0, 64.0)),
                            );
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new("Nothing scanned yet")
                                    .color(p.text)
                                    .size(15.0)
                                    .family(bold()),
                            );
                            ui.label(
                                RichText::new("Press Scan to find reclaimable space")
                                    .color(p.muted)
                                    .size(12.5),
                            );
                        });
                    });
                    return;
                }

                let filter = self.category_filter;
                let filtered: Vec<usize> = self
                    .targets
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| filter.is_none_or(|c| t.category == c))
                    .map(|(i, _)| i)
                    .collect();
                let max_size = filtered
                    .iter()
                    .map(|&i| self.targets[i].size)
                    .max()
                    .unwrap_or(1)
                    .max(1);
                let targets = &mut self.targets;

                egui::Frame::default()
                    .fill(p.card)
                    .stroke(Stroke::new(1.0, p.card_stroke))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(10))
                    .outer_margin(Margin {
                        left: 8,
                        right: 12,
                        top: 6,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        TableBuilder::new(ui)
                            .striped(true)
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .column(Column::exact(30.0))
                            .column(Column::exact(130.0))
                            .column(Column::exact(64.0))
                            .column(Column::remainder().clip(true).at_least(140.0))
                            .column(Column::remainder().clip(true).at_least(140.0))
                            .header(24.0, |mut header| {
                                for title in ["", "SIZE", "SAFETY", "PATH", "REASON"] {
                                    header.col(|ui| {
                                        ui.label(
                                            RichText::new(title).size(10.0).color(p.muted).strong(),
                                        );
                                    });
                                }
                            })
                            .body(|body| {
                                body.rows(32.0, filtered.len(), |mut row| {
                                    let t = &mut targets[filtered[row.index()]];
                                    row.col(|ui| {
                                        ui.checkbox(&mut t.selected, "");
                                    });
                                    row.col(|ui| {
                                        Self::size_cell(ui, p, t.size, max_size, t.safety);
                                    });
                                    row.col(|ui| {
                                        let (label, color) = match t.safety {
                                            Safety::Regenerable => ("safe", p.green),
                                            Safety::Review => ("review", p.amber),
                                        };
                                        Self::pill(ui, label, color);
                                    });
                                    row.col(|ui| {
                                        if let Action::Command(spec) = t.action {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(format!("$ {}", spec.display))
                                                        .monospace()
                                                        .size(12.0)
                                                        .color(p.teal),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(spec.display);
                                        } else {
                                            let p_str = t.path.display().to_string();
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&p_str)
                                                        .monospace()
                                                        .size(12.0)
                                                        .color(p.text),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(p_str);
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&t.reason).size(11.5).color(p.muted),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&t.reason);
                                    });
                                });
                            });
                    });
            });
    }
}
