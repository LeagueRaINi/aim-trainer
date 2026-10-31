//! Hit sounds, synthesized at startup so the game ships without asset files.

use std::{f32::consts::TAU, sync::Arc};

use bevy::{audio::Volume, prelude::*};

use crate::settings::{AudioSettings, HitSound};

const SAMPLE_RATE: u32 = 44_100;

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sounds);
    }
}

#[derive(Resource)]
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
        commands.spawn((
            AudioPlayer(handle.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(audio.volume)),
        ));
    }
}

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |samples: Vec<f32>| sources.add(AudioSource { bytes: Arc::from(wav(&samples)) });
    commands.insert_resource(HitSounds {
        tick: add(synth(0.05, |t| {
            // Bright, very short click: what most shooters use for body hits. Scaled so the two
            // partials (which peak together around 1 ms) never clip.
            0.75 * ((TAU * 3200.0 * t).sin() * (-t * 110.0).exp() + 0.5 * (TAU * 5100.0 * t).sin() * (-t * 220.0).exp())
        })),
        pop: add(synth(0.12, |t| {
            // Falling pitch "bubble pop"; phase is the integral of 250 + 900·e^(−25t) Hz.
            let phase = TAU * (250.0 * t + 900.0 / 25.0 * (1.0 - (-25.0 * t).exp()));
            phase.sin() * (-t * 32.0).exp()
        })),
        ding: add(synth(0.45, |t| {
            ((TAU * 1568.0 * t).sin() + 0.45 * (TAU * 2349.0 * t).sin() + 0.2 * (TAU * 3136.0 * t).sin())
                * (-t * 11.0).exp()
                * 0.6
        })),
    });
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
