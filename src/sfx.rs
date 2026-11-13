//! Hit sounds, synthesized at startup so the game ships without asset files. Every one is short
//! and leaves headroom: kills can come 150 ms apart (Gridshot), and a sound still ringing when
//! the next starts smears and, summed, clips.
//!
//! Each is built the way a struck thing sounds, in layers: a millisecond or two of filtered
//! noise for the strike, a tone whose pitch moves as it dies for the body, and something lower
//! for weight. Bare sine tones, however short, are beeps. Their energy sits between 0.7 and
//! 2.5 kHz, below the 3 to 4 kHz where the ear is most sensitive and a sound heard hundreds of
//! times a session turns piercing, and their gains make them equally loud (A-weighted), so
//! changing the sound doesn't change the volume.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::settings::{AudioSettings, HitSound};

const SAMPLE_RATE: u32 = 44_100;

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sounds);
    }
}

#[derive(Resource, Default)]
pub struct HitSounds {
    tick: Handle<AudioSource>,
    pop: Handle<AudioSource>,
    ding: Handle<AudioSource>,
    snap: Handle<AudioSource>,
}

impl HitSounds {
    pub fn play(&self, commands: &mut Commands, audio: &AudioSettings) {
        let handle = match audio.hit_sound {
            HitSound::Off => return,
            HitSound::Tick => &self.tick,
            HitSound::Pop => &self.pop,
            HitSound::Ding => &self.ding,
            HitSound::Snap => &self.snap,
        };
        commands
            .spawn((AudioPlayer(handle.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(audio.volume))));
    }
}

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |samples: Vec<f32>| sources.add(AudioSource { bytes: Arc::from(wav(&samples)) });
    commands.insert_resource(HitSounds { tick: add(tick()), pop: add(pop()), ding: add(ding()), snap: add(snap()) });
}

/// A knock on wood: the default. A tone that drops from 1.5 to 0.9 kHz within a few
/// milliseconds, a shorter one an octave and a half above it, a soft thump below 300 Hz for
/// weight and a millisecond of noise for the strike.
fn tick() -> Vec<f32> {
    let click = band_pass(noise(samples_in(0.07), 0x9E37_79B9), 3200.0, 1.0);
    shaped(0.07, 0.986, |i, t| {
        let body = 0.62 * glide(t, 900.0, 1500.0, 0.005).sin() * fall(t, 0.010);
        let knock = 0.26 * glide(t, 2350.0, 2900.0, 0.004).sin() * fall(t, 0.005);
        let thump = 0.20 * glide(t, 170.0, 300.0, 0.010).sin() * fall(t, 0.013);
        (1.3 * (1.6 * click[i] * fall(t, 0.0012) + body + knock + thump)).tanh()
    })
}

/// A bubble bursting: one round tone that rises from 0.5 to 1.15 kHz as it dies, with a little
/// of its octave, swelling in over a millisecond or two instead of starting with a strike.
fn pop() -> Vec<f32> {
    let click = band_pass(noise(samples_in(0.08), 0x51ED_270B), 2400.0, 0.8);
    shaped(0.08, 0.898, |i, t| {
        let rise = glide(t, 1150.0, 520.0, 0.010);
        let swell = (1.0 - fall(t, 0.0015)) * fall(t, 0.014);
        0.95 * (rise.sin() + 0.22 * (2.0 * rise).sin()) * swell + 0.7 * click[i] * fall(t, 0.0008)
    })
}

/// A small piece of metal struck: the partials of a ringing bar (1, 2.76 and 5.4 times
/// 1.48 kHz, the higher the shorter) over a quiet one an octave below, and a noise strike.
/// Silent well within 150 ms, so one ding is over before the next.
fn ding() -> Vec<f32> {
    const PITCH: f32 = 1480.0;
    let strike = band_pass(noise(samples_in(0.15), 0xB529_7A4D), 5200.0, 0.9);
    shaped(0.15, 0.781, |i, t| {
        let partial = |ratio: f32, level: f32, lasts: f32| level * (TAU * PITCH * ratio * t).sin() * fall(t, lasts);
        let bar = partial(1.0, 1.0, 0.034)
            + partial(2.756, 0.42, 0.016)
            + partial(5.404, 0.16, 0.006)
            + partial(0.5, 0.22, 0.024);
        (0.75 * bar + 1.2 * strike[i] * fall(t, 0.0015)).tanh()
    })
}

/// A dry crack, like a shooter's hit marker: mostly noise, around 2.3 kHz with some air above
/// it, over a short tone that drops to 500 Hz. The shortest of the four.
fn snap() -> Vec<f32> {
    let crack = band_pass(noise(samples_in(0.06), 0x68E3_1DA4), 2300.0, 0.7);
    let air = band_pass(noise(samples_in(0.06), 0x1B56_C4E9), 6000.0, 0.8);
    shaped(0.06, 0.914, |i, t| {
        let body = 0.55 * glide(t, 520.0, 1100.0, 0.005).sin() * fall(t, 0.008);
        (1.6 * (1.7 * crack[i] * fall(t, 0.004) + 0.9 * air[i] * fall(t, 0.0018) + body)).tanh()
    })
}

fn samples_in(secs: f32) -> usize {
    (secs * SAMPLE_RATE as f32) as usize
}

/// A sound of `secs` from its layers: `f` gives sample `i` at time `t`, which is faded in over
/// half a millisecond and out over five (so playback never starts or ends with a click of its
/// own) and scaled by `gain`.
fn shaped(secs: f32, gain: f32, f: impl Fn(usize, f32) -> f32) -> Vec<f32> {
    (0..samples_in(secs))
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let fade_in = (t / 0.0005).min(1.0);
            let fade_out = ((secs - t) / 0.005).min(1.0);
            gain * f(i, t) * fade_in * fade_out
        })
        .collect()
}

/// How much is left at `t` of something that dies away with the time constant `lasts`.
fn fall(t: f32, lasts: f32) -> f32 {
    (-t / lasts).exp()
}

/// The phase at `t` of a tone that glides from `from` Hz to `to` Hz with the time constant
/// `glides`: the integral of its frequency.
fn glide(t: f32, to: f32, from: f32, glides: f32) -> f32 {
    TAU * (to * t + (from - to) * glides * (1.0 - fall(t, glides)))
}

/// White noise in −1..1, the same every time (xorshift from `seed`): a build always makes the
/// same sounds.
fn noise(count: usize, seed: u32) -> Vec<f32> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / 2_147_483_648.0 - 1.0
        })
        .collect()
}

/// What a two-pole band-pass filter around `freq` Hz lets through (unity gain at `freq`; a
/// higher `q` is a narrower band): noise turned into a strike with a colour.
fn band_pass(mut samples: Vec<f32>, freq: f32, q: f32) -> Vec<f32> {
    let turn = TAU * freq / SAMPLE_RATE as f32;
    let alpha = turn.sin() / (2.0 * q);
    let (feed, back_1, back_2) =
        (alpha / (1.0 + alpha), -2.0 * turn.cos() / (1.0 + alpha), (1.0 - alpha) / (1.0 + alpha));
    let (mut in_1, mut in_2, mut out_1, mut out_2) = (0.0, 0.0, 0.0, 0.0);
    for sample in &mut samples {
        let out = feed * (*sample - in_2) - back_1 * out_1 - back_2 * out_2;
        (in_2, in_1, out_2, out_1) = (in_1, *sample, out_1, out);
        *sample = out;
    }
    samples
}

/// Encodes mono samples in −1..=1 as a 16-bit PCM WAV file.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kills 150 ms apart: each sound has died away (−30 dB) by the next, and the sounds played
    /// back to back never sum past full scale.
    #[test]
    fn sounds_fit_rapid_kills() {
        let gap = (0.15 * SAMPLE_RATE as f32) as usize;
        let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        for (name, samples) in [("tick", tick()), ("pop", pop()), ("ding", ding()), ("snap", snap())] {
            let (loudest, tail) = (peak(&samples), peak(samples.get(gap..).unwrap_or_default()));
            assert!(tail <= loudest * 10f32.powf(-30.0 / 20.0), "{name} still at {tail} of {loudest} after 150 ms");
            let mut mix = vec![0.0; 3 * gap + samples.len()];
            for start in [0, gap, 2 * gap] {
                mix[start..start + samples.len()].iter_mut().zip(&samples).for_each(|(m, s)| *m += s);
            }
            assert!(peak(&mix) <= 1.0, "{name} clips at {} when repeated", peak(&mix));
        }
    }
}
