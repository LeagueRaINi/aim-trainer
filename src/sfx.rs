//! Hit sounds, synthesized at startup so the game ships without asset files. Every one is short
//! and leaves headroom: kills can come 150 ms apart (Gridshot), and a sound still ringing when
//! the next starts smears and, summed, clips.

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
}

impl HitSounds {
    pub fn play(&self, commands: &mut Commands, audio: &AudioSettings) {
        let handle = match audio.hit_sound {
            HitSound::Off => return,
            HitSound::Tick => &self.tick,
            HitSound::Pop => &self.pop,
            HitSound::Ding => &self.ding,
        };
        commands
            .spawn((AudioPlayer(handle.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(audio.volume))));
    }
}

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |samples: Vec<f32>| sources.add(AudioSource { bytes: Arc::from(wav(&samples)) });
    commands.insert_resource(HitSounds { tick: add(tick()), pop: add(pop()), ding: add(ding()) });
}

/// Bright, very short click: what most shooters use for body hits. Scaled so the two partials
/// (which peak together around 1 ms) never clip.
fn tick() -> Vec<f32> {
    synth(0.05, |t| {
        0.75 * ((TAU * 3200.0 * t).sin() * (-t * 110.0).exp() + 0.5 * (TAU * 5100.0 * t).sin() * (-t * 220.0).exp())
    })
}

/// A quick drop in pitch, like a bubble bursting: phase is the integral of 650 + 1400·e^(−90t) Hz,
/// so the sweep is over in about 30 ms and nothing low is left to boom.
fn pop() -> Vec<f32> {
    synth(0.07, |t| {
        let phase = TAU * (650.0 * t + 1400.0 / 90.0 * (1.0 - (-90.0 * t).exp()));
        0.7 * phase.sin() * (-t * 75.0).exp()
    })
}

/// A short, high bell: two partials a fifth apart plus a bright inharmonic one that dies first.
/// Damped to silence within ~150 ms, so successive dings don't beat against each other.
fn ding() -> Vec<f32> {
    synth(0.16, |t| {
        let ring = (TAU * 2637.0 * t).sin() + 0.4 * (TAU * 3951.0 * t).sin();
        let strike = 0.3 * (TAU * 6850.0 * t).sin() * (-t * 90.0).exp();
        0.45 * (ring * (-t * 30.0).exp() + strike)
    })
}

/// Samples `f` for `secs`, with a 1 ms fade-in and a 5 ms fade-out so playback never starts or
/// ends with a click.
fn synth(secs: f32, f: impl Fn(f32) -> f32) -> Vec<f32> {
    let count = (secs * SAMPLE_RATE as f32) as usize;
    (0..count)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let fade_in = (t / 0.001).min(1.0);
            let fade_out = ((secs - t) / 0.005).min(1.0);
            f(t) * fade_in * fade_out
        })
        .collect()
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
        for (name, samples) in [("tick", tick()), ("pop", pop()), ("ding", ding())] {
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
