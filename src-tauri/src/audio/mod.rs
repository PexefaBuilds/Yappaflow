//! Microphone capture on a dedicated thread (cpal streams are !Send).
//! Captures at the device's native rate, downmixes to mono f32; resampling to
//! 16 kHz happens on stop (a few ms per 10 s of audio).

pub mod resample;
pub mod wav;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Raw capture result (native rate, mono).
pub struct Captured {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub type LevelFn = Arc<dyn Fn(f32) + Send + Sync>;

enum Cmd {
    Start { device: Option<String>, reply: Sender<Result<()>> },
    Stop { reply: Sender<Option<Captured>> },
    Cancel,
}

#[derive(Clone)]
pub struct Recorder {
    tx: Sender<Cmd>,
}

impl Recorder {
    /// `on_level` receives a 0..1 peak about every 50 ms while recording.
    pub fn spawn(on_level: LevelFn) -> Result<Self> {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("audio".into()).spawn(move || audio_thread(rx, on_level))?;
        Ok(Self { tx })
    }

    pub fn start(&self, device: Option<String>) -> Result<()> {
        let (reply, r) = channel();
        self.tx.send(Cmd::Start { device, reply }).map_err(|_| anyhow!("audio thread gone"))?;
        r.recv_timeout(Duration::from_secs(3)).map_err(|_| anyhow!("mic start timed out"))?
    }

    pub fn stop(&self) -> Option<Captured> {
        let (reply, r) = channel();
        self.tx.send(Cmd::Stop { reply }).ok()?;
        r.recv_timeout(Duration::from_secs(3)).ok().flatten()
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(Cmd::Cancel);
    }
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices().map(|it| it.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

struct Active {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

fn audio_thread(rx: Receiver<Cmd>, on_level: LevelFn) {
    let mut active: Option<Active> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start { device, reply } => {
                active = None; // drop any previous stream
                match open(device.as_deref(), on_level.clone()) {
                    Ok(a) => {
                        active = Some(a);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::Stop { reply } => {
                let out = active.take().map(|a| {
                    drop(a._stream); // stop callbacks before taking the buffer
                    let samples = std::mem::take(&mut *a.buf.lock().unwrap());
                    Captured { samples, sample_rate: a.rate }
                });
                let _ = reply.send(out);
            }
            Cmd::Cancel => active = None,
        }
    }
}

fn find_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        if let Ok(mut devs) = host.input_devices() {
            if let Some(d) = devs.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                return Ok(d);
            }
        }
        log::warn!("mic '{name}' not found, using default");
    }
    host.default_input_device().context("no input device (microphone) found")
}

fn open(device: Option<&str>, on_level: LevelFn) -> Result<Active> {
    let dev = find_device(device)?;
    let cfg = dev.default_input_config().context("mic has no default config")?;
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    let buf = Arc::new(Mutex::new(Vec::<f32>::with_capacity(rate as usize * 30)));
    let stream_cfg: cpal::StreamConfig = cfg.clone().into();

    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&dev, &stream_cfg, channels, buf.clone(), on_level)?,
        cpal::SampleFormat::I16 => build::<i16>(&dev, &stream_cfg, channels, buf.clone(), on_level)?,
        cpal::SampleFormat::U16 => build::<u16>(&dev, &stream_cfg, channels, buf.clone(), on_level)?,
        cpal::SampleFormat::I32 => build::<i32>(&dev, &stream_cfg, channels, buf.clone(), on_level)?,
        f => return Err(anyhow!("unsupported mic sample format {f:?}")),
    };
    stream.play().context("failed to start mic stream")?;
    log::info!("mic '{}' @ {rate} Hz x{channels}", dev.name().unwrap_or_default());
    Ok(Active { _stream: stream, buf, rate })
}

fn build<T>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    channels: usize,
    buf: Arc<Mutex<Vec<f32>>>,
    on_level: LevelFn,
) -> Result<cpal::Stream>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let mut last_emit = Instant::now();
    let mut peak = 0f32;
    let stream = dev.build_input_stream(
        cfg,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut b = buf.lock().unwrap();
            for frame in data.chunks(channels) {
                let s = frame.iter().map(|x| x.to_sample::<f32>()).sum::<f32>() / channels as f32;
                peak = peak.max(s.abs());
                b.push(s);
            }
            drop(b);
            if last_emit.elapsed() >= Duration::from_millis(50) {
                on_level(peak.min(1.0));
                peak = 0.0;
                last_emit = Instant::now();
            }
        },
        |e| log::error!("mic stream error: {e}"),
        None,
    )?;
    Ok(stream)
}
