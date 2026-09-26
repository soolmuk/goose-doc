//! egui panel. Renders the state model in `panel::mod` and forwards input back
//! as `Command`s.
//!
//! All UI text is English: egui's default font has no CJK glyphs, so non-Latin
//! labels would render as blank boxes.

use eframe::egui;
use std::time::{Duration, Instant};

use super::{bind_choices, client_hint, validate, Command, Panel, PanelState, Reach};

pub struct PanelApp {
    panel: Panel,
    bind_text: String,
    port_text: String,
    message: Option<String>,
    copied_at: Option<Instant>,
}

impl PanelApp {
    pub fn new(panel: Panel) -> Self {
        let bind_text = panel.settings.bind.clone();
        let port_text = panel.settings.port.to_string();
        Self {
            panel,
            bind_text,
            port_text,
            message: None,
            copied_at: None,
        }
    }
}

impl eframe::App for PanelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.panel.tick();

        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("goose-doc");
                ui.separator();
                ui.label("Serve goose documentation for the goose-doc-guide skill");
            });
        });

        egui::Panel::bottom("footer").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(message) = &self.message {
                    ui.colored_label(egui::Color32::from_rgb(200, 80, 60), message);
                }
                if self
                    .copied_at
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(3))
                {
                    ui.colored_label(egui::Color32::from_rgb(60, 140, 80), "Copied");
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.server_section(ui);
            ui.add_space(8.0);
            ui.separator();
            self.settings_section(ui);
            ui.add_space(8.0);
            ui.separator();
            self.status_section(ui);
        });

        // Keep uptime and request count moving while a server runs.
        if self.panel.state.is_running() {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Closing the window must not leave a listening socket behind.
        self.panel.shutdown();
    }
}

impl PanelApp {
    fn server_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let running = self.panel.state.is_running();

            if ui
                .add_enabled(!running, egui::Button::new("Start"))
                .clicked()
            {
                self.sync_settings_from_fields();
                self.panel.apply(Command::Start);
                if let PanelState::Running(status) = &self.panel.state {
                    if self.panel.settings.open_browser {
                        let _ = open::that(&status.url);
                    }
                }
            }

            if ui.add_enabled(running, egui::Button::new("Stop")).clicked() {
                self.panel.apply(Command::Stop);
                self.message = None;
            }

            ui.separator();

            match &self.panel.state {
                PanelState::Idle => {
                    ui.label("Not running");
                }
                PanelState::Running(_) => {
                    ui.colored_label(Color::ok(), "Running");
                }
                PanelState::Stopped { .. } => {
                    ui.label("Stopped");
                }
                PanelState::Failed(_) => {
                    ui.colored_label(Color::error(), "Error");
                }
            }
        });

        if let PanelState::Failed(message) = self.panel.state.clone() {
            ui.colored_label(Color::error(), message);
        }
    }

    fn settings_section(&mut self, ui: &mut egui::Ui) {
        let running = self.panel.state.is_running();

        ui.add_enabled_ui(!running, |ui| {
            egui::Grid::new("settings")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Bind address");
                    egui::ComboBox::from_id_salt("bind")
                        .width(280.0)
                        .selected_text(self.bind_text.clone())
                        .show_ui(ui, |ui| {
                            for choice in bind_choices() {
                                ui.selectable_value(&mut self.bind_text, choice.clone(), choice);
                            }
                        });
                    ui.end_row();

                    ui.label("Port");
                    ui.add(egui::TextEdit::singleline(&mut self.port_text).desired_width(120.0));
                    ui.end_row();

                    ui.label("Docs root");
                    ui.label(
                        self.panel
                            .settings
                            .docs_dir
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "cached bundle".to_string()),
                    );
                    ui.end_row();

                    ui.label("Open browser");
                    ui.checkbox(&mut self.panel.settings.open_browser, "");
                    ui.end_row();
                });
        });

        if !running && ui.button("Validate").clicked() {
            self.sync_settings_from_fields();
            self.message = match validate(&self.panel.settings) {
                Ok(()) => Some("Settings look valid".to_string()),
                Err(message) => Some(message),
            };
        }
    }

    fn status_section(&mut self, ui: &mut egui::Ui) {
        let PanelState::Running(status) = self.panel.state.clone() else {
            if !matches!(self.panel.state, PanelState::Failed(_)) {
                ui.label("Start the server to see status here.");
            }
            return;
        };

        egui::Grid::new("status")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("URL");
                ui.horizontal(|ui| {
                    ui.monospace(&status.url);
                    if ui.small_button("Open").clicked() {
                        let _ = open::that(&status.url);
                    }
                });
                ui.end_row();

                ui.label("goose command");
                ui.horizontal(|ui| {
                    let hint = client_hint(&status);
                    ui.monospace(&hint);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(hint);
                        self.copied_at = Some(Instant::now());
                    }
                });
                ui.end_row();

                ui.label("Listening");
                ui.monospace(&status.listening);
                ui.end_row();

                ui.label("Reach");
                ui.label(match status.reach {
                    Reach::Network => "all interfaces (other machines can connect)",
                    Reach::ThisMachineOnly => "this machine only",
                    Reach::Single => "one interface",
                });
                ui.end_row();

                ui.label("Version");
                ui.label(
                    status
                        .docs_version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                ui.end_row();

                ui.label("Pages");
                ui.label(status.docs_pages.to_string());
                ui.end_row();

                ui.label("Docs root");
                ui.monospace(status.docs_path.display().to_string());
                ui.end_row();

                ui.label("Uptime");
                ui.label(format_uptime(status.uptime));
                ui.end_row();

                ui.label("Requests");
                ui.label(status.requests.to_string());
                ui.end_row();
            });

        if matches!(status.reach, Reach::ThisMachineOnly) {
            ui.add_space(4.0);
            ui.colored_label(
                Color::warn(),
                "This machine only. Other machines cannot connect; \
                 choose 0.0.0.0 to serve the network.",
            );
        }
    }

    /// Copy the text fields into settings, reporting a bad port instead of
    /// silently reverting it.
    fn sync_settings_from_fields(&mut self) {
        self.panel.settings.bind = self.bind_text.trim().to_string();
        match self.port_text.trim().parse::<u16>() {
            Ok(port) => {
                self.panel.settings.port = port;
                self.message = None;
            }
            Err(_) => {
                self.message = Some(format!("\"{}\" is not a valid port", self.port_text.trim()));
                self.port_text = self.panel.settings.port.to_string();
            }
        }
    }
}

struct Color;

impl Color {
    fn ok() -> egui::Color32 {
        egui::Color32::from_rgb(60, 140, 80)
    }

    fn error() -> egui::Color32 {
        egui::Color32::from_rgb(200, 80, 60)
    }

    fn warn() -> egui::Color32 {
        egui::Color32::from_rgb(190, 140, 40)
    }
}

fn format_uptime(uptime: Duration) -> String {
    let total = uptime.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_is_formatted_for_humans() {
        assert_eq!(format_uptime(Duration::from_secs(5)), "5s");
        assert_eq!(format_uptime(Duration::from_secs(65)), "1m 5s");
        assert_eq!(format_uptime(Duration::from_secs(3725)), "1h 2m 5s");
    }
}
