//! Aircraft sound playback: sound.cfg engine layers driven by an estimated
//! RPM, plus stall horn, wind, ground roll, flap motor and touchdown sounds.
//! The RPM estimate is a playable approximation; the flight model has no
//! engine RPM state yet.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fsx_vehicle::sound::{Sounds, Viewpoint};
use fsx_vehicle::wav::Wave;
use std::path::Path;
use std::sync::{Arc, Mutex};

struct Voice {
    wave: Arc<Wave>,
    position: f64,
    pitch: f64,
    target: f32,
    gain: f32,
    looped: bool,
}
impl Voice {
    fn new(wave: Arc<Wave>, looped: bool) -> Self {
        Self {
            wave,
            position: 0.,
            pitch: 1.,
            target: 0.,
            gain: 0.,
            looped,
        }
    }
    fn next(&mut self, output_rate: f64) -> f32 {
        // Per-sample smoothing removes zipper noise from 60 Hz control updates.
        self.gain += (self.target - self.gain) * 0.002;
        let n = self.wave.samples.len();
        if n == 0 || (self.gain < 1e-4 && self.target == 0.) {
            return 0.;
        }
        if self.position >= n as f64 {
            if !self.looped {
                return 0.;
            }
            self.position %= n as f64;
        }
        let i = self.position as usize;
        let t = (self.position - i as f64) as f32;
        let a = self.wave.samples[i];
        let b = if i + 1 < n {
            self.wave.samples[i + 1]
        } else if self.looped {
            self.wave.samples[0]
        } else {
            0.
        };
        self.position += self.pitch * f64::from(self.wave.rate) / output_rate;
        (a + (b - a) * t) * self.gain
    }
}
struct Mix {
    voices: Vec<Voice>,
    oneshots: Vec<Voice>,
}
struct Engine {
    voice: usize,
    layer: fsx_vehicle::sound::Layer,
}
/// Per-frame aircraft state the sound mix depends on.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cue {
    pub throttle: f64,
    pub airspeed: f64,
    pub cruise_speed: f64,
    pub on_ground: bool,
    pub ground_speed: f64,
    pub stalled: bool,
    pub flaps_moving: bool,
    pub cockpit: bool,
    pub paused: bool,
    /// Downward speed at the moment the wheels touch, if they just did.
    pub touchdown: Option<f64>,
}
pub struct Audio {
    _stream: cpal::Stream,
    mix: Arc<Mutex<Mix>>,
    engine: Vec<Engine>,
    named: [Option<usize>; 4],
    touchdown: Vec<Arc<Wave>>,
    rpm: f64,
    touchdowns: usize,
}
const STALL: usize = 0;
const WIND: usize = 1;
const ROLL: usize = 2;
const FLAPS: usize = 3;
impl Audio {
    pub fn new(sound_folder: &Path, windows_1252: bool) -> Result<Self, String> {
        let sounds = Sounds::load(sound_folder, windows_1252)?;
        let mut cache: Vec<(std::path::PathBuf, Arc<Wave>)> = Vec::new();
        let mut wave = |path: &Path| -> Result<Arc<Wave>, String> {
            if let Some((_, w)) = cache.iter().find(|(p, _)| p == path) {
                return Ok(w.clone());
            }
            let w = Arc::new(Wave::load(path)?);
            cache.push((path.to_path_buf(), w.clone()));
            Ok(w)
        };
        let mut voices = Vec::new();
        let mut engine = Vec::new();
        for layer in sounds.engine {
            engine.push(Engine {
                voice: voices.len(),
                layer: layer.clone(),
            });
            voices.push(Voice::new(wave(&layer.file)?, true));
        }
        let mut named = [None; 4];
        for (slot, path) in [
            (STALL, &sounds.stall_warning),
            (WIND, &sounds.wind),
            (ROLL, &sounds.ground_roll),
            (FLAPS, &sounds.flaps),
        ] {
            if let Some(path) = path {
                named[slot] = Some(voices.len());
                voices.push(Voice::new(wave(path)?, true));
            }
        }
        let touchdown = sounds
            .touchdown
            .iter()
            .map(|p| wave(p))
            .collect::<Result<_, _>>()?;
        let mix = Arc::new(Mutex::new(Mix {
            voices,
            oneshots: Vec::new(),
        }));
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err("Audio output does not use 32-bit float samples".into());
        }
        let channels = usize::from(config.channels());
        let rate = f64::from(config.sample_rate().0);
        let shared = mix.clone();
        let stream = device
            .build_output_stream(
                &config.into(),
                move |out: &mut [f32], _| {
                    let Ok(mut guard) = shared.lock() else {
                        out.fill(0.);
                        return;
                    };
                    let m = &mut *guard;
                    for frame in out.chunks_mut(channels) {
                        let mut sum = 0.;
                        for v in m.voices.iter_mut().chain(m.oneshots.iter_mut()) {
                            sum += v.next(rate);
                        }
                        frame.fill((sum * 0.35).clamp(-1., 1.));
                    }
                    m.oneshots
                        .retain(|v| v.position < v.wave.samples.len() as f64);
                },
                |e| eprintln!("Audio stream error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            mix,
            engine,
            named,
            touchdown,
            rpm: 0.25,
            touchdowns: 0,
        })
    }
    pub fn update(&mut self, cue: Cue, elapsed: f64) {
        // Idle about 0.25 of rated RPM, ~0.85 static full power, approaching
        // rated RPM as the propeller unloads with airspeed.
        let unload = (cue.airspeed / cue.cruise_speed.max(1.)).clamp(0., 1.3);
        let target =
            (0.25 + 0.6 * cue.throttle + 0.12 * unload * (0.3 + 0.7 * cue.throttle)).clamp(0., 1.);
        self.rpm += (target - self.rpm) * (elapsed / 0.8).min(1.);
        let Ok(mut m) = self.mix.lock() else { return };
        let mute = if cue.paused { 0. } else { 1. };
        for e in &self.engine {
            let audible = match e.layer.viewpoint {
                Viewpoint::Both => true,
                Viewpoint::Inside => cue.cockpit,
                Viewpoint::Outside => !cue.cockpit,
            };
            let v = &mut m.voices[e.voice];
            v.pitch = e.layer.pitch_at(self.rpm);
            v.target = if audible {
                e.layer.volume_at(self.rpm) as f32 * mute
            } else {
                0.
            };
        }
        let mut set = |slot: usize, gain: f64, pitch: f64| {
            if let Some(i) = self.named[slot] {
                m.voices[i].target = (gain * mute as f64) as f32;
                m.voices[i].pitch = pitch;
            }
        };
        set(
            STALL,
            if cue.stalled && !cue.on_ground {
                0.8
            } else {
                0.
            },
            1.,
        );
        set(
            WIND,
            (cue.airspeed / 70.).clamp(0., 1.) * 0.5,
            0.8 + 0.4 * (cue.airspeed / 70.).min(1.),
        );
        let roll = if cue.on_ground {
            (cue.ground_speed / 30.).clamp(0., 1.)
        } else {
            0.
        };
        set(ROLL, roll * 0.7, 0.7 + 0.5 * roll);
        set(FLAPS, if cue.flaps_moving { 0.6 } else { 0. }, 1.);
        if let (Some(sink), false) = (cue.touchdown, self.touchdown.is_empty() || cue.paused) {
            let mut v = Voice::new(
                self.touchdown[self.touchdowns % self.touchdown.len()].clone(),
                false,
            );
            self.touchdowns += 1;
            v.gain = (0.3 + sink / 3.).clamp(0.3, 1.) as f32;
            v.target = v.gain;
            m.oneshots.push(v);
        }
    }
}
