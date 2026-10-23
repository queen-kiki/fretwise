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
    /// Channels on the open output, for the channel-pair picker.
    pub output_channels: u16,
    /// Under ASIO, the one device both streams open. cpal keeps a driver's
    /// buffers per `Device` value and sets them up for input and output
    /// together; streams opened from two lookups of the same driver each
    /// recreate the buffers, and the first one goes silent.
    asio_device: Option<cpal::Device>,
    /// Bumped by [`AudioEngine::chime`]; the output callback plays one chime
    /// per increment it sees.
    chimes: Arc<AtomicU32>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(SharedPitch::default()),
            amp: Arc::new(SharedAmp::default()),
            // Room for ~85 ms at 96 kHz; the output side keeps only a small
            // cushion of it queued (see `EffectChain::process_from_ring`).
            ring: Arc::new(SampleRing::new(8192)),
            stream: None,
            info: None,
            output: None,
            output_channels: 0,
            asio_device: None,
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

    pub fn output_devices(host: HostId) -> Vec<String> {
        cpal::host_from_id(host)
            .ok()
            .and_then(|h| h.output_devices().ok())
            .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn default_input(host: HostId) -> Option<String> {
        cpal::host_from_id(host).ok()?.default_input_device()?.name().ok()
    }

    pub fn stop(&mut self) {
        self.stop_input();
        self.output = None;
        self.asio_device = None;
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

    /// Open `device` (or the host's default) for the effect chain and chimes,
    /// playing on its channels `pair * 2` and the one after (clamped to the
    /// device; other channels stay silent). Call [`AudioEngine::play`] once
    /// the input is open too.
    pub fn start_output(&mut self, host: HostId, device: Option<&str>, pair: usize) -> Result<(), String> {
        self.output = None;
        self.output_channels = 0;
        let dev = match device {
            Some(name) => self.open(host, name, true).ok_or_else(|| format!("output not found: {name}"))?,
            None => cpal::host_from_id(host)
                .map_err(|e| e.to_string())?
                .default_output_device()
                .ok_or("no output device")?,
        };
        let supported = dev.default_output_config().map_err(|e| e.to_string())?;
        let config: StreamConfig = supported.config();
        let sr = config.sample_rate.0 as f32;
        let mut chain = EffectChain::new(sr, 512);
        chain.push(Box::new(CleanAmp::new(sr, self.amp.clone())));
        chain.push(Box::new(Cabinet::new(sr, self.amp.clone())));
        let channels = config.channels.max(1) as usize;
        let parts = OutputParts {
            chime: Chime::new(sr),
            chimes: self.chimes.clone(),
            chain,
            ring: self.ring.clone(),
            amp: self.amp.clone(),
            first: (pair * 2).min(channels - 1),
        };

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_output::<f32>(&dev, &config, parts),
            SampleFormat::I32 => build_output::<i32>(&dev, &config, parts),
            SampleFormat::I16 => build_output::<i16>(&dev, &config, parts),
            SampleFormat::U16 => build_output::<u16>(&dev, &config, parts),
            other => return Err(format!("unsupported output format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        self.output = Some(stream);
        self.output_channels = config.channels;
        Ok(())
    }

    /// Open `device` to analyze one of its input channels (0-based). Call
    /// [`AudioEngine::play`] to start it.
    pub fn start(&mut self, host: HostId, device: &str, channel: usize) -> Result<(), String> {
        self.stop_input();
        let dev = self.open(host, device, false).ok_or_else(|| format!("device not found: {device}"))?;
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

        self.info = Some(StreamInfo {
            sample_rate: config.sample_rate.0,
            channels: config.channels,
            format: supported.sample_format(),
        });
        self.stream = Some(stream);
        Ok(())
    }
}

impl AudioEngine {
    /// Start whichever streams are open: output, then input. Errors are
    /// (output, input).
    ///
    /// Both must be built first: under ASIO, opening the second stream sets
    /// the driver's buffers up again for both, which drivers only allow while
    /// stopped.
    pub fn play(&self) -> (Option<String>, Option<String>) {
        let play = |s: &Option<cpal::Stream>| s.as_ref().and_then(|s| s.play().err()).map(|e| e.to_string());
        (play(&self.output), play(&self.stream))
    }

    /// The device called `name`, as an output or an input. Under ASIO it's
    /// the same `Device` value for both (see `asio_device`).
    fn open(&mut self, host: HostId, name: &str, output: bool) -> Option<cpal::Device> {
        let lookup = if output { find_output_device } else { find_device };
        if !is_asio(host) {
            return lookup(host, name);
        }
        if let Some(dev) = &self.asio_device {
            if dev.name().is_ok_and(|n| n == name) {
                return Some(dev.clone());
            }
        }
        let dev = lookup(host, name)?;
        self.asio_device = Some(dev.clone());
        Some(dev)
    }
}

/// ASIO loads one driver at a time, and its input and output share it.
pub fn is_asio(host: HostId) -> bool {
    host.name() == "ASIO"
}

fn find_output_device(host: HostId, name: &str) -> Option<cpal::Device> {
    cpal::host_from_id(host)
        .ok()?
        .output_devices()
        .ok()?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
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
            ring.note_block(data.len() / channels);
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )
}

/// What the output callback owns.
struct OutputParts {
    chime: Chime,
    /// Bumped once per chime to play; see [`AudioEngine::chime`].
    chimes: Arc<AtomicU32>,
    chain: EffectChain,
    ring: Arc<SampleRing>,
    amp: Arc<SharedAmp>,
    /// The first of the two channels to play on.
    first: usize,
}

fn build_output<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    parts: OutputParts,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let OutputParts { mut chime, chimes, mut chain, ring, amp, first } = parts;
    let channels = config.channels.max(1) as usize;
    // The pair, or just the last channel when the device has an odd one out.
    let lanes = first..(first + 2).min(channels);
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
                let frame = &mut data[frame_idx * channels..(frame_idx + 1) * channels];
                frame.fill(T::EQUILIBRIUM);
                frame[lanes.clone()].fill(T::from_sample(mixed));
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
