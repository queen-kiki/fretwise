//! Audio input via cpal. This is the part a plugin replaces: a host would call
//! `Analyzer::process` from its own `process()` instead of us opening a device.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, HostId, SampleFormat, SizedSample, StreamConfig};
use fretwise_core::{Analyzer, AnalyzerConfig, SharedPitch};

pub struct StreamInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
}

pub struct AudioEngine {
    pub shared: Arc<SharedPitch>,
    stream: Option<cpal::Stream>,
    pub info: Option<StreamInfo>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self { shared: Arc::new(SharedPitch::default()), stream: None, info: None }
    }

    pub fn hosts() -> Vec<HostId> {
        cpal::available_hosts()
    }

    pub fn input_devices(host: HostId) -> Vec<String> {
        cpal::host_from_id(host)
            .ok()
            .and_then(|h| h.input_devices().ok())
            .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn default_input(host: HostId) -> Option<String> {
        cpal::host_from_id(host).ok()?.default_input_device()?.name().ok()
    }

    /// Input channel count of a device, for the channel picker.
    pub fn channel_count(host: HostId, device: &str) -> u16 {
        find_device(host, device)
            .and_then(|d| d.default_input_config().ok())
            .map_or(0, |c| c.channels())
    }

    pub fn stop(&mut self) {
        self.stream = None;
        self.info = None;
    }

    /// Open `device` and analyze one of its input channels (0-based).
    pub fn start(&mut self, host: HostId, device: &str, channel: usize) -> Result<(), String> {
        self.stop();
        let dev = find_device(host, device).ok_or_else(|| format!("device not found: {device}"))?;
        let supported = dev.default_input_config().map_err(|e| e.to_string())?;
        let config: StreamConfig = supported.config();
        let channel = channel.min(config.channels.saturating_sub(1) as usize);
        let analyzer = Analyzer::new(config.sample_rate.0 as f32, AnalyzerConfig::default(), self.shared.clone());

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&dev, &config, analyzer, channel),
            SampleFormat::I32 => build::<i32>(&dev, &config, analyzer, channel),
            SampleFormat::I16 => build::<i16>(&dev, &config, analyzer, channel),
            SampleFormat::U16 => build::<u16>(&dev, &config, analyzer, channel),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;

        self.info = Some(StreamInfo {
            sample_rate: config.sample_rate.0,
            channels: config.channels,
            format: supported.sample_format(),
        });
        self.stream = Some(stream);
        Ok(())
    }
}

fn find_device(host: HostId, name: &str) -> Option<cpal::Device> {
    cpal::host_from_id(host)
        .ok()?
        .input_devices()
        .ok()?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut analyzer: Analyzer,
    channel: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            for frame in data.chunks_exact(channels) {
                analyzer.push(frame[channel].to_sample::<f32>());
            }
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )
}
