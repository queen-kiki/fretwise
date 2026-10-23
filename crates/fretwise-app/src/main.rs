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
    /// Output device; `None` is the driver's default.
    output: Option<String>,
    outputs: Vec<String>,
    /// 0-based output channel pair: 0 is channels 1-2.
    output_pair: usize,
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
        let (device, channel, output, output_pair) = match (saved_host, audio.device) {
            (Some(_), Some(d)) => (Some(d), audio.channel, audio.output, audio.output_pair),
            _ => (AudioEngine::default_input(host), 0, None, 0),
        };
        let mut audio = AudioSetup {
            engine: AudioEngine::new(),
            host,
            device,
            devices: AudioEngine::input_devices(host),
            channel,
            output,
            outputs: AudioEngine::output_devices(host),
            output_pair,
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
                output: self.audio.output.clone(),
                output_pair: self.audio.output_pair,
            },
            ui: self.ui.state(),
            ..Default::default()
        };
        self.store.save(&state);
    }
}

impl AudioSetup {
    /// ASIO loads one driver at a time, so there the output is the input
    /// device itself (its "default" would be whichever driver is listed first).
    fn shares_device(&self) -> bool {
        audio::is_asio(self.host)
    }

    /// The output to open: the chosen one, the driver's default, or under
    /// ASIO the input device.
    fn output_device(&self) -> Option<&str> {
        if self.shares_device() {
            self.device.as_deref()
        } else {
            self.output.as_deref()
        }
    }

    fn restart(&mut self) {
        self.error = None;
        if self.shares_device() {
            // Both streams come from one driver session: close it entirely
            // and open input and output on it afresh.
            self.engine.stop();
        }
        let output = self.output_device().map(str::to_owned);
        self.output_error = self.engine.start_output(self.host, output.as_deref(), self.output_pair).err();
        let pairs = self.engine.output_channels.div_ceil(2) as usize;
        if pairs > 0 {
            self.output_pair = self.output_pair.min(pairs - 1);
        }
        match self.device.clone() {
            Some(device) => {
                if let Err(e) = self.engine.start(self.host, &device, self.channel) {
                    self.error = Some(e);
                }
                // From the open stream, not a second lookup: under ASIO that
                // would be another handle on the driver.
                self.channel_count = self.engine.info.as_ref().map_or(0, |i| i.channels);
                if self.channel_count > 0 {
                    self.channel = self.channel.min(self.channel_count as usize - 1);
                }
            }
            None => {
                self.engine.stop_input();
                self.error = Some("no input device".into());
            }
        }
        let (output_error, input_error) = self.engine.play();
        self.output_error = self.output_error.take().or(output_error);
        self.error = self.error.take().or(input_error);
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
                            self.outputs = AudioEngine::output_devices(h);
                            self.output = None;
                            self.output_pair = 0;
                            changed = true;
                        }
                    }
                });
            ui.end_row();

            ui.label("Input");
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
                    self.outputs = AudioEngine::output_devices(self.host);
                    changed = true;
                }
            });
            ui.end_row();

            // Input jacks as buttons, the way they're numbered on the box.
            ui.label("Channel");
            ui.horizontal_wrapped(|ui| {
                for c in 0..self.channel_count.max(1) as usize {
                    changed |= ui.selectable_value(&mut self.channel, c, format!(" {} ", c + 1)).changed();
                }
            });
            ui.end_row();

            ui.label("Output");
            let shared = self.shares_device();
            let shown = self.output_device().unwrap_or("System default").to_owned();
            let picker = ui.add_enabled_ui(!shared, |ui| {
                egui::ComboBox::from_id_salt("output")
                    .selected_text(shown)
                    .width(140.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        let before = self.output.clone();
                        ui.selectable_value(&mut self.output, None, "System default");
                        for d in &self.outputs {
                            ui.selectable_value(&mut self.output, Some(d.clone()), d);
                        }
                        if self.output != before {
                            self.output_pair = 0;
                            changed = true;
                        }
                    });
            });
            if shared {
                picker.response.on_disabled_hover_text("ASIO runs one driver at a time: sound goes out the input's device");
            }
            ui.end_row();

            // Output jacks in stereo pairs, as the box labels them; a lone
            // last channel gets its own button.
            ui.label("Channel");
            ui.horizontal_wrapped(|ui| {
                let channels = self.engine.output_channels.max(2) as usize;
                for p in 0..channels.div_ceil(2) {
                    let label = if p * 2 + 1 < channels {
                        format!(" {}-{} ", p * 2 + 1, p * 2 + 2)
                    } else {
                        format!(" {} ", p * 2 + 1)
                    };
                    changed |= ui.selectable_value(&mut self.output_pair, p, label).changed();
                }
            });
            ui.end_row();
        });

        match (&self.error, &self.engine.info) {
            (Some(e), _) => {
                ui.colored_label(fretwise_ui::BAD, e);
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
        self.audio.engine.shared.set_gate_db(self.ui.gate_db());
        self.audio.engine.amp.store(self.ui.amp());
        self.ui.set_out_level_db(self.audio.engine.amp.out_level_db());
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
    // Scales, chords, and decks first: loading the saved state looks them up by id.
    // Extra files can go in %APPDATA%\Fretwise\{scales,chords,decks}\.
    let user_scales = dirs::config_dir().map(|d| d.join("Fretwise").join("scales"));
    for e in fretwise_core::scale::init(user_scales.as_deref()) {
        eprintln!("skipping scale {e}");
    }
    let user_chords = dirs::config_dir().map(|d| d.join("Fretwise").join("chords"));
    for e in fretwise_core::chord::init(user_chords.as_deref()) {
        eprintln!("skipping chord {e}");
    }
    let user_decks = dirs::config_dir().map(|d| d.join("Fretwise").join("decks"));
    for e in fretwise_core::deck::init(user_decks.as_deref()) {
        eprintln!("skipping deck {e}");
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
