//! Audio input and output via cpal. This is the part a plugin replaces: a host
//! would call `Analyzer::process` and `EffectChain::process` from its own
//! `process()` instead of us opening devices.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, HostId, SampleFormat, SizedSample, StreamConfig};
use fretwise_core::{Analyzer, AnalyzerConfig, Cabinet, Chime, CleanAmp, EffectChain, SampleRing, SharedAmp, SharedPitch};

pub struct StreamInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
}

pub struct AudioEngine {
    pub shared: Arc<SharedPitch>,
    pub amp: Arc<SharedAmp>,
    ring: Arc<SampleRing>,
    stream: Option<cpal::Stream>,
    pub info: Option<StreamInfo>,
    output: Option<cpal::Stream>,
    /// Bumped by [`AudioEngine::chime`]; the output callback plays one chime
    /// per increment it sees.
    chimes: Arc<AtomicU32>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(SharedPitch::default()),
            amp: Arc::new(SharedAmp::default()),
            // 4096-sample ring with a ~16 ms (768-sample @ 48 kHz) ceiling so
            // live guitar monitoring stays tight even if input starts first.
            ring: Arc::new(SampleRing::new(4096, 768)),
            stream: None,
            info: None,
            output: None,
            chimes: Arc::new(AtomicU32::new(0)),
        }
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
        self.stop_input();
        self.output = None;
    }

    pub fn stop_input(&mut self) {
        self.stream = None;
        self.info = None;
        self.ring.clear();
        self.ring.set_sample_rate(0);
    }

    pub fn chime(&self) {
        self.chimes.fetch_add(1, Ordering::Relaxed);
    }

    /// Open the host's default output for the effect chain and chimes.
    pub fn start_output(&mut self, host: HostId) -> Result<(), String> {
        self.output = None;
        let dev = cpal::host_from_id(host)
            .map_err(|e| e.to_string())?
            .default_output_device()
            .ok_or("no output device")?;
        let supported = dev.default_output_config().map_err(|e| e.to_string())?;
        let config: StreamConfig = supported.config();
        let sr = config.sample_rate.0 as f32;
        let chime = Chime::new(sr);
        let mut chain = EffectChain::new(sr, 512);
        chain.push(Box::new(CleanAmp::new(sr, self.amp.clone())));
        chain.push(Box::new(Cabinet::new(sr, self.amp.clone())));
        let ring = self.ring.clone();
        let amp = self.amp.clone();

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_output::<f32>(&dev, &config, chime, self.chimes.clone(), chain, ring, amp),
            SampleFormat::I32 => build_output::<i32>(&dev, &config, chime, self.chimes.clone(), chain, ring, amp),
            SampleFormat::I16 => build_output::<i16>(&dev, &config, chime, self.chimes.clone(), chain, ring, amp),
            SampleFormat::U16 => build_output::<u16>(&dev, &config, chime, self.chimes.clone(), chain, ring, amp),
            other => return Err(format!("unsupported output format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.output = Some(stream);
        Ok(())
    }

    /// Open `device` and analyze one of its input channels (0-based).
    pub fn start(&mut self, host: HostId, device: &str, channel: usize) -> Result<(), String> {
        self.stop_input();
        let dev = find_device(host, device).ok_or_else(|| format!("device not found: {device}"))?;
        let supported = dev.default_input_config().map_err(|e| e.to_string())?;
        let config: StreamConfig = supported.config();
        let channel = channel.min(config.channels.saturating_sub(1) as usize);
        let analyzer = Analyzer::new(config.sample_rate.0 as f32, AnalyzerConfig::default(), self.shared.clone());
        // The output may run at another rate; the effect chain resamples from this one.
        self.ring.set_sample_rate(config.sample_rate.0);
        let ring = self.ring.clone();

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&dev, &config, analyzer, ring, channel),
            SampleFormat::I32 => build::<i32>(&dev, &config, analyzer, ring, channel),
            SampleFormat::I16 => build::<i16>(&dev, &config, analyzer, ring, channel),
            SampleFormat::U16 => build::<u16>(&dev, &config, analyzer, ring, channel),
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
    ring: Arc<SampleRing>,
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
                let s = frame[channel].to_sample::<f32>();
                analyzer.push(s);
                ring.push(s);
            }
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )
}

fn build_output<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut chime: Chime,
    chimes: Arc<AtomicU32>,
    mut chain: EffectChain,
    ring: Arc<SampleRing>,
    amp: Arc<SharedAmp>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    let mut seen = chimes.load(Ordering::Relaxed);
    let mut env_sq = 0.0_f32;
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let count = chimes.load(Ordering::Relaxed);
            if count != seen {
                seen = count;
                chime.trigger();
            }
            let total_frames = data.len() / channels;
            let out_gain = 10.0_f32.powf(amp.load().out_gain_db / 20.0);
            let mut sum_sq = 0.0_f32;
            chain.process_from_ring(&ring, total_frames, |frame_idx, chain_sample| {
                let final_sample = (chain_sample * out_gain).clamp(-1.0, 1.0);
                sum_sq += final_sample * final_sample;
                let mixed = (final_sample + chime.next_sample()).clamp(-1.0, 1.0);
                let out = T::from_sample(mixed);
                let start = frame_idx * channels;
                data[start..start + channels].fill(out);
            });
            let block_sq = if total_frames > 0 { sum_sq / total_frames as f32 } else { 0.0 };
            env_sq = if block_sq > env_sq {
                block_sq
            } else {
                env_sq * 0.85 + block_sq * 0.15
            };
            amp.set_out_level_db(10.0 * env_sq.max(1e-12).log10());
        },
        |err| eprintln!("audio output error: {err}"),
        None,
    )
}
