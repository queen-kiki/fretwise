#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod state;

use audio::AudioEngine;
use cpal::HostId;
use fretwise_ui::FretwiseUi;
use state::{AudioSettings, SavedState, Store};

struct App {
    audio: AudioEngine,
    ui: FretwiseUi,
    host: HostId,
    device: Option<String>,
    devices: Vec<String>,
    channel: usize,
    channel_count: u16,
    error: Option<String>,
    store: Store,
    last_save: f64,
}

impl App {
    fn new() -> Self {
        let (store, saved) = Store::open();
        let audio = saved.audio;
        let saved_host = AudioEngine::hosts()
            .into_iter()
            .find(|h| Some(h.name()) == audio.host.as_deref());
        let host = saved_host.unwrap_or_else(|| cpal::default_host().id());
        // Keep the saved device even if it's unplugged right now: it shows as
        // "device not found" until it's back and ⟳ is pressed, rather than
        // being forgotten in favour of the default.
        let (device, channel) = match (saved_host, audio.device) {
            (Some(_), Some(d)) => (Some(d), audio.channel),
            _ => (AudioEngine::default_input(host), 0),
        };
        let mut app = Self {
            audio: AudioEngine::new(),
            ui: FretwiseUi::from_state(saved.ui),
            host,
            device,
            devices: AudioEngine::input_devices(host),
            channel,
            channel_count: 0,
            error: None,
            store,
            last_save: 0.0,
        };
        app.restart();
        app
    }

    fn save(&mut self) {
        let state = SavedState {
            audio: AudioSettings {
                host: Some(self.host.name().to_string()),
                device: self.device.clone(),
                channel: self.channel,
            },
            ui: self.ui.state(),
            ..Default::default()
        };
        self.store.save(&state);
    }

    fn restart(&mut self) {
        self.error = None;
        let Some(device) = self.device.clone() else {
            self.audio.stop();
            self.error = Some("no input device".into());
            return;
        };
        self.channel_count = AudioEngine::channel_count(self.host, &device);
        if self.channel_count > 0 {
            self.channel = self.channel.min(self.channel_count as usize - 1);
        }
        if let Err(e) = self.audio.start(self.host, &device, self.channel) {
            self.error = Some(e);
        }
    }

    fn device_bar(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Driver")
                .selected_text(self.host.name())
                .show_ui(ui, |ui| {
                    for h in AudioEngine::hosts() {
                        if ui.selectable_value(&mut self.host, h, h.name()).changed() {
                            // Release the device before switching driver models.
                            self.audio.stop();
                            self.devices = AudioEngine::input_devices(h);
                            self.device = AudioEngine::default_input(h).or_else(|| self.devices.first().cloned());
                            self.channel = 0;
                            changed = true;
                        }
                    }
                });

            egui::ComboBox::from_label("Input")
                .selected_text(self.device.as_deref().unwrap_or("none"))
                .width(260.0)
                .show_ui(ui, |ui| {
                    for d in &self.devices {
                        if ui.selectable_value(&mut self.device, Some(d.clone()), d).changed() {
                            self.channel = 0;
                            changed = true;
                        }
                    }
                });

            egui::ComboBox::from_label("Channel")
                .selected_text(format!("{}", self.channel + 1))
                .show_ui(ui, |ui| {
                    for c in 0..self.channel_count.max(1) as usize {
                        changed |= ui.selectable_value(&mut self.channel, c, format!("{}", c + 1)).changed();
                    }
                });

            if ui.button("⟳").on_hover_text("Rescan devices").clicked() {
                self.devices = AudioEngine::input_devices(self.host);
                changed = true;
            }

            fretwise_ui::show_level(ui, &self.audio.shared.load());

            match (&self.error, &self.audio.info) {
                (Some(e), _) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
                (None, Some(i)) => {
                    ui.weak(format!("{} Hz · {} ch · {:?}", i.sample_rate, i.channels, i.format));
                }
                _ => {}
            }
        });
        if changed {
            self.restart();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("devices").show(ctx, |ui| {
            ui.add_space(4.0);
            self.device_bar(ui);
            ui.add_space(2.0);
        });
        let reading = self.audio.shared.load();
        egui::CentralPanel::default().show(ctx, |ui| self.ui.show(ui, reading));

        // Saving is skipped when nothing changed, so checking often is cheap.
        let now = ctx.input(|i| i.time);
        if now - self.last_save >= 1.0 || ctx.input(|i| i.viewport().close_requested()) {
            self.last_save = now;
            self.save();
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.save();
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fretwise")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([800.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("Fretwise", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
