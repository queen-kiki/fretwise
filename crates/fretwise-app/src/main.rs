#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod state;

use audio::AudioEngine;
use cpal::HostId;
use fretwise_ui::FretwiseUi;
use state::{AudioSettings, SavedState, Store};

/// The input device and how it's doing: the app's part of the settings
/// panel.
struct AudioSetup {
    engine: AudioEngine,
    host: HostId,
    device: Option<String>,
    devices: Vec<String>,
    channel: usize,
    channel_count: u16,
    error: Option<String>,
    output_error: Option<String>,
}

struct App {
    audio: AudioSetup,
    ui: FretwiseUi,
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
        let mut audio = AudioSetup {
            engine: AudioEngine::new(),
            host,
            device,
            devices: AudioEngine::input_devices(host),
            channel,
            channel_count: 0,
            error: None,
            output_error: None,
        };
        audio.restart();
        Self {
            audio,
            ui: FretwiseUi::from_state(saved.ui),
            store,
            last_save: 0.0,
        }
    }

    fn save(&mut self) {
        let state = SavedState {
            audio: AudioSettings {
                host: Some(self.audio.host.name().to_string()),
                device: self.audio.device.clone(),
                channel: self.audio.channel,
            },
            ui: self.ui.state(),
            ..Default::default()
        };
        self.store.save(&state);
    }
}

impl AudioSetup {
    fn restart(&mut self) {
        self.error = None;
        // Chimes go to the driver's default output, whatever the input is.
        self.output_error = self.engine.start_output(self.host).err();
        let Some(device) = self.device.clone() else {
            self.engine.stop_input();
            self.error = Some("no input device".into());
            return;
        };
        self.channel_count = AudioEngine::channel_count(self.host, &device);
        if self.channel_count > 0 {
            self.channel = self.channel.min(self.channel_count as usize - 1);
        }
        if let Err(e) = self.engine.start(self.host, &device, self.channel) {
            self.error = Some(e);
        }
    }
}

impl fretwise_ui::Host for AudioSetup {
    fn warning(&self) -> Option<String> {
        self.error.clone()
    }

    fn audio_settings(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        egui::Grid::new("audio").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Driver");
            egui::ComboBox::from_id_salt("driver")
                .selected_text(self.host.name())
                .width(170.0)
                .show_ui(ui, |ui| {
                    for h in AudioEngine::hosts() {
                        if ui.selectable_value(&mut self.host, h, h.name()).changed() {
                            // Release the device before switching driver models.
                            self.engine.stop();
                            self.devices = AudioEngine::input_devices(h);
                            self.device = AudioEngine::default_input(h).or_else(|| self.devices.first().cloned());
                            self.channel = 0;
                            changed = true;
                        }
                    }
                });
            ui.end_row();

            ui.label("Device");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("input")
                    .selected_text(self.device.as_deref().unwrap_or("none"))
                    .width(140.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for d in &self.devices {
                            if ui.selectable_value(&mut self.device, Some(d.clone()), d).changed() {
                                self.channel = 0;
                                changed = true;
                            }
                        }
                    });
                if ui.button("⟳").on_hover_text("Rescan devices").clicked() {
                    self.devices = AudioEngine::input_devices(self.host);
                    changed = true;
                }
            });
            ui.end_row();

            // Input jacks as buttons, the way they're numbered on the box.
            ui.label("Input");
            ui.horizontal_wrapped(|ui| {
                for c in 0..self.channel_count.max(1) as usize {
                    changed |= ui.selectable_value(&mut self.channel, c, format!(" {} ", c + 1)).changed();
                }
            });
            ui.end_row();
        });

        match (&self.error, &self.engine.info) {
            (Some(e), _) => {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
            }
            (None, Some(i)) => {
                ui.weak(format!("{} Hz · {} ch · {:?}", i.sample_rate, i.channels, i.format));
            }
            _ => {}
        }
        if let Some(e) = &self.output_error {
            ui.weak(format!("no sound: {e}"));
        }
        if changed {
            self.restart();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let reading = self.audio.engine.shared.load();
        let frame = egui::Frame::central_panel(&ctx.style()).inner_margin(14);
        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| self.ui.show(ui, reading, &mut self.audio));
        if self.ui.take_chime() {
            self.audio.engine.chime();
        }

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
    // Scales first: loading the saved state looks them up by id.
    // Extra scales can go in %APPDATA%\Fretwise\scales\, one JSON file each.
    let user_scales = dirs::config_dir().map(|d| d.join("Fretwise").join("scales"));
    for e in fretwise_core::scale::init(user_scales.as_deref()) {
        eprintln!("skipping scale {e}");
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fretwise")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([800.0, 560.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/logo/fretwise-logo-256.png"))
                    .expect("the bundled logo is a valid PNG"),
            ),
        ..Default::default()
    };
    eframe::run_native("Fretwise", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
