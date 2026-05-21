//! Native procedural sound-effects synthesizer.
//!
//! Rust equivalent of `js/modules/audio/sound-defs.js` (port-plan §4,
//! documented fallback to baked samples). Synthesizes all arcade SFX as
//! PCM-16 mono WAV byte buffers at startup — no external asset files, no
//! `rand` crate. Noise is produced by a deterministic 64-bit LCG. All audio
//! is encoded at 44 100 Hz and registered as `AudioSource` assets via
//! `bevy::audio`.

use std::sync::Arc;

use bevy::audio::AudioSource;
use bevy::prelude::*;

use crate::components::Faction;
use crate::messages::{Damage, Death, Fire};

// ── WAV encoder ─────────────────────────────────────────────────────────────

/// Encode a slice of `f32` samples (range [-1, 1]) as a standard RIFF/WAVE
/// PCM-16 mono file. Produces a 44-byte header followed by 2 bytes per sample.
fn wav_pcm16_mono(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len() as u32;
    let byte_rate = sample_rate * 2; // 1 channel * 2 bytes/sample
    let data_bytes = num_samples * 2;
    let file_size = 36 + data_bytes; // RIFF chunk size = 36 + data

    let mut buf: Vec<u8> = Vec::with_capacity(44 + data_bytes as usize);

    // ── RIFF chunk descriptor ──
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes()); // ChunkSize
    buf.extend_from_slice(b"WAVE");

    // ── fmt sub-chunk ──
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (PCM = 16)
    buf.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat: PCM = 1
    buf.extend_from_slice(&1u16.to_le_bytes()); // NumChannels: mono
    buf.extend_from_slice(&sample_rate.to_le_bytes()); // SampleRate
    buf.extend_from_slice(&byte_rate.to_le_bytes()); // ByteRate
    buf.extend_from_slice(&2u16.to_le_bytes()); // BlockAlign: 1 ch * 2 bytes
    buf.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample

    // ── data sub-chunk ──
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let pcm = (clamped * i16::MAX as f32) as i16;
        buf.extend_from_slice(&pcm.to_le_bytes());
    }

    buf
}

// ── Synth primitives ─────────────────────────────────────────────────────────

const SAMPLE_RATE: u32 = 44_100;
const SR: f32 = SAMPLE_RATE as f32;

/// Sine wave at `freq` Hz, starting at `phase` radians. Returns (sample, next_phase).
#[inline(always)]
fn sine(phase: f32) -> f32 {
    phase.sin()
}

/// Square wave (duty 50 %) at the given phase.
#[inline(always)]
fn square(phase: f32) -> f32 {
    if (phase % (2.0 * std::f32::consts::PI)) < std::f32::consts::PI {
        1.0
    } else {
        -1.0
    }
}

/// Sawtooth wave at the given phase.
#[inline(always)]
fn saw(phase: f32) -> f32 {
    let t = (phase % (2.0 * std::f32::consts::PI)) / (2.0 * std::f32::consts::PI);
    2.0 * t - 1.0
}

/// Deterministic white noise using a 64-bit LCG (Knuth multiplier).
/// Returns (sample ∈ [-1,1], next_state).
#[inline(always)]
fn lcg_noise(state: u64) -> (f32, u64) {
    // Knuth MMIX LCG: multiplier 6364136223846793005, addend 1442695040888963407
    let next = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let sample = ((next >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0;
    (sample, next)
}

/// Exponential amplitude envelope. Returns an amplitude multiplier in [0,1].
/// `t` = sample index, `attack_samples` = number of samples to ramp up,
/// `decay_coeff` = per-sample multiplier for the exponential tail (e.g. `exp(-k/SR)`).
#[inline(always)]
fn env_exp(t: usize, attack_samples: usize, decay_coeff: f32) -> f32 {
    if t < attack_samples {
        t as f32 / attack_samples as f32
    } else {
        decay_coeff.powi((t - attack_samples) as i32)
    }
}

/// Linear amplitude envelope (straight ramp down over `total_samples`).
#[allow(dead_code)]
#[inline(always)]
fn env_linear(t: usize, total_samples: usize) -> f32 {
    1.0 - (t as f32 / total_samples as f32)
}

/// Compute the instantaneous frequency for a linear sweep from `start_hz` to
/// `end_hz` at sample index `t` over `total_samples`.
#[inline(always)]
fn sweep_freq(start_hz: f32, end_hz: f32, t: usize, total_samples: usize) -> f32 {
    let alpha = t as f32 / total_samples as f32;
    start_hz + (end_hz - start_hz) * alpha
}

/// Accumulate phase: given current `phase` and `freq`, advance by one sample
/// and return the new phase (wrapped to avoid float drift).
#[inline(always)]
fn advance_phase(phase: f32, freq: f32) -> f32 {
    (phase + 2.0 * std::f32::consts::PI * freq / SR) % (2.0 * std::f32::consts::PI)
}

// ── SFX synthesis ────────────────────────────────────────────────────────────

/// Player shoot — quick descending square/saw "pew": 880 → 220 Hz sweep, ~0.08 s.
fn synth_player_shoot() -> Vec<u8> {
    let duration = 0.08_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.05) as usize; // 5 % attack
    let decay_coeff = (-7.0_f32 / SR).exp(); // fast exponential decay

    let mut samples = Vec::with_capacity(n);
    let mut phase: f32 = 0.0;
    for i in 0..n {
        let freq = sweep_freq(880.0, 220.0, i, n);
        let amp = env_exp(i, attack, decay_coeff);
        // Mix square + saw for a nasal "pew" character
        let sig = 0.6 * square(phase) + 0.4 * saw(phase);
        samples.push(sig * amp * 0.45);
        phase = advance_phase(phase, freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Enemy shoot — lower, softer blip: 440 → 110 Hz sweep, ~0.07 s.
fn synth_enemy_shoot() -> Vec<u8> {
    let duration = 0.07_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.04) as usize;
    let decay_coeff = (-6.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut phase: f32 = 0.0;
    for i in 0..n {
        let freq = sweep_freq(440.0, 110.0, i, n);
        let amp = env_exp(i, attack, decay_coeff);
        // Softer: pure saw, lower amplitude so it doesn't dominate
        let sig = saw(phase);
        samples.push(sig * amp * 0.22);
        phase = advance_phase(phase, freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Explosion — filtered noise burst + low sine thump, ~0.4 s.
fn synth_explosion() -> Vec<u8> {
    let duration = 0.40_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.01) as usize;
    // Slow decay for noise layer; fast for the thump
    let noise_decay = (-3.5_f32 / SR).exp();
    let thump_decay = (-10.0_f32 / SR).exp();
    let thump_freq = 55.0_f32;

    let mut samples = Vec::with_capacity(n);
    let mut noise_state: u64 = 0xDEAD_BEEF_1234_5678;
    let mut thump_phase: f32 = 0.0;

    // A simple one-pole low-pass filter state for the noise (softens it to
    // give a more "boom"-like quality vs pure white noise).
    let mut lp: f32 = 0.0;
    let lp_coeff = 0.15_f32; // cutoff factor — lower = darker

    for i in 0..n {
        let (raw_noise, next_state) = lcg_noise(noise_state);
        noise_state = next_state;

        // Low-pass filter the noise
        lp = lp + lp_coeff * (raw_noise - lp);

        let noise_amp = env_exp(i, attack, noise_decay);
        let thump_amp = env_exp(i, attack, thump_decay);

        let noise_sig = lp * noise_amp * 0.4;
        let thump_sig = sine(thump_phase) * thump_amp * 0.25;

        samples.push((noise_sig + thump_sig).clamp(-1.0, 1.0));
        thump_phase = advance_phase(thump_phase, thump_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Player hit — harsh noise/buzz + low tone, ~0.15 s.
fn synth_player_hit() -> Vec<u8> {
    let duration = 0.15_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.02) as usize;
    let decay_coeff = (-5.0_f32 / SR).exp();
    let buzz_freq = 180.0_f32;

    let mut samples = Vec::with_capacity(n);
    let mut noise_state: u64 = 0xABCD_EF01_2345_6789;
    let mut buzz_phase: f32 = 0.0;

    for i in 0..n {
        let (noise, next_state) = lcg_noise(noise_state);
        noise_state = next_state;

        let amp = env_exp(i, attack, decay_coeff);
        // Raw unfiltered noise for harshness + a buzzy square at 180 Hz
        let sig = 0.5 * noise + 0.5 * square(buzz_phase);
        samples.push(sig * amp * 0.40);
        buzz_phase = advance_phase(buzz_phase, buzz_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Pickup — bright rising sine "ding": 660 → 1320 Hz sweep, ~0.12 s.
fn synth_pickup() -> Vec<u8> {
    let duration = 0.12_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.08) as usize; // slightly longer attack = "ding" character
    let decay_coeff = (-6.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut phase: f32 = 0.0;
    for i in 0..n {
        let freq = sweep_freq(660.0, 1320.0, i, n);
        let amp = env_exp(i, attack, decay_coeff);
        let sig = sine(phase);
        samples.push(sig * amp * 0.38);
        phase = advance_phase(phase, freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

// ── Resource ─────────────────────────────────────────────────────────────────

/// Holds `Handle<AudioSource>` for every arcade SFX.
/// Inserted at startup by `setup_sfx`; read by the playback systems.
#[derive(Resource)]
pub struct Sfx {
    pub player_shoot: Handle<AudioSource>,
    pub enemy_shoot: Handle<AudioSource>,
    pub explosion: Handle<AudioSource>,
    pub player_hit: Handle<AudioSource>,
    pub pickup: Handle<AudioSource>,
}

// ── Setup system ─────────────────────────────────────────────────────────────

/// Synthesize all SFX, register them as `AudioSource` assets, and insert the
/// `Sfx` resource. Wire into `Startup` before the first frame.
pub fn setup_sfx(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let make = |bytes: Vec<u8>| AudioSource {
        bytes: Arc::from(bytes.as_slice()),
    };

    let sfx = Sfx {
        player_shoot: sources.add(make(synth_player_shoot())),
        enemy_shoot: sources.add(make(synth_enemy_shoot())),
        explosion: sources.add(make(synth_explosion())),
        player_hit: sources.add(make(synth_player_hit())),
        pickup: sources.add(make(synth_pickup())),
    };
    commands.insert_resource(sfx);
}

// ── Playback systems ─────────────────────────────────────────────────────────

/// Spawn an `AudioPlayer` entity for every `Fire` message. Caps at 4 per
/// faction per frame to prevent audio spam on rapid auto-fire.
pub fn play_shoot(
    mut commands: Commands,
    sfx: Res<Sfx>,
    mut fire: MessageReader<Fire>,
) {
    let mut player_count = 0u32;
    let mut enemy_count = 0u32;
    const CAP: u32 = 4;

    for ev in fire.read() {
        match ev.faction {
            Faction::Player => {
                if player_count < CAP {
                    commands.spawn((
                        AudioPlayer::new(sfx.player_shoot.clone()),
                        PlaybackSettings::DESPAWN,
                    ));
                    player_count += 1;
                }
            }
            Faction::Enemy => {
                if enemy_count < CAP {
                    commands.spawn((
                        AudioPlayer::new(sfx.enemy_shoot.clone()),
                        PlaybackSettings::DESPAWN,
                    ));
                    enemy_count += 1;
                }
            }
        }
    }
}

/// Spawn an explosion sound for every `Death` message (capped at 6 per frame).
pub fn play_explosion(
    mut commands: Commands,
    sfx: Res<Sfx>,
    mut deaths: MessageReader<Death>,
) {
    const CAP: u32 = 6;
    let mut count = 0u32;
    for _ev in deaths.read() {
        if count < CAP {
            commands.spawn((
                AudioPlayer::new(sfx.explosion.clone()),
                PlaybackSettings::DESPAWN,
            ));
            count += 1;
        }
    }
}

/// Play the player-hit sound whenever the player entity receives `Damage`.
pub fn play_player_hit(
    mut commands: Commands,
    sfx: Res<Sfx>,
    mut dmg: MessageReader<Damage>,
    players: Query<(), With<crate::components::Ship>>,
) {
    for ev in dmg.read() {
        if players.get(ev.target).is_ok() {
            commands.spawn((
                AudioPlayer::new(sfx.player_hit.clone()),
                PlaybackSettings::DESPAWN,
            ));
        }
    }
}
