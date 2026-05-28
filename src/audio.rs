//! Native procedural sound-effects synthesizer + WAV file player.
//!
//! At startup the system scans `<crate-root>/sfx/` for `*.wav` files,
//! reads them into `AudioSource` assets, and groups them by event key
//! (the stem with the `_v<N>` variant suffix stripped).  Playback systems
//! prefer the real recordings and fall back to the procedural synth when
//! no file matches.
//!
//! Public surface kept stable so `app.rs` registration is untouched:
//!   - resource  `Sfx`
//!   - systems   `setup_sfx` / `play_shoot` / `play_explosion` / `play_player_hit`
//!
//! Throttle: the same event key is not replayed within 30 ms (mirrors
//! `SOUND_THROTTLE_MS = 30` from `audio-manager.js`).
//!
//! Master volume for file-based SFX: 0.8 linear.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;

use crate::components::Faction;
use crate::messages::{Crit, Damage, Death, Fire, Reaction, ReactionFx};

// ── WAV encoder ─────────────────────────────────────────────────────────────

/// Encode a slice of `f32` samples (range [-1, 1]) as a standard RIFF/WAVE
/// PCM-16 mono file.  Produces a 44-byte header followed by 2 bytes per sample.
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
    buf.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat: PCM = 1
    buf.extend_from_slice(&1u16.to_le_bytes());  // NumChannels: mono
    buf.extend_from_slice(&sample_rate.to_le_bytes()); // SampleRate
    buf.extend_from_slice(&byte_rate.to_le_bytes());   // ByteRate
    buf.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign: 1 ch * 2 bytes
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

/// Sine wave at `freq` Hz, starting at `phase` radians.
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
    let next = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let sample = ((next >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0;
    (sample, next)
}

/// Exponential amplitude envelope.
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

/// Compute the instantaneous frequency for a linear sweep.
#[inline(always)]
fn sweep_freq(start_hz: f32, end_hz: f32, t: usize, total_samples: usize) -> f32 {
    let alpha = t as f32 / total_samples as f32;
    start_hz + (end_hz - start_hz) * alpha
}

/// Advance oscillator phase by one sample at `freq` Hz.
#[inline(always)]
fn advance_phase(phase: f32, freq: f32) -> f32 {
    (phase + 2.0 * std::f32::consts::PI * freq / SR) % (2.0 * std::f32::consts::PI)
}

// ── SFX synthesis ────────────────────────────────────────────────────────────

/// Player shoot — quick descending square/saw "pew": 880 → 220 Hz, ~0.08 s.
fn synth_player_shoot() -> Vec<u8> {
    let duration = 0.08_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.05) as usize;
    let decay_coeff = (-7.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut phase: f32 = 0.0;
    for i in 0..n {
        let freq = sweep_freq(880.0, 220.0, i, n);
        let amp = env_exp(i, attack, decay_coeff);
        let sig = 0.6 * square(phase) + 0.4 * saw(phase);
        samples.push(sig * amp * 0.45);
        phase = advance_phase(phase, freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Enemy shoot — lower, softer blip: 440 → 110 Hz, ~0.07 s.
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
    let noise_decay = (-3.5_f32 / SR).exp();
    let thump_decay = (-10.0_f32 / SR).exp();
    let thump_freq = 55.0_f32;

    let mut samples = Vec::with_capacity(n);
    let mut noise_state: u64 = 0xDEAD_BEEF_1234_5678;
    let mut thump_phase: f32 = 0.0;
    let mut lp: f32 = 0.0;
    let lp_coeff = 0.15_f32;

    for i in 0..n {
        let (raw_noise, next_state) = lcg_noise(noise_state);
        noise_state = next_state;
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
        let sig = 0.5 * noise + 0.5 * square(buzz_phase);
        samples.push(sig * amp * 0.40);
        buzz_phase = advance_phase(buzz_phase, buzz_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Pickup — bright rising sine "ding": 660 → 1320 Hz, ~0.12 s.
fn synth_pickup() -> Vec<u8> {
    let duration = 0.12_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.08) as usize;
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

/// Shatter — an icy glass "crack": bright sine sweep 2400 → 700 Hz + a noise
/// edge, ~0.16 s (CRYO reaction).
fn synth_shatter() -> Vec<u8> {
    let duration = 0.16_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.01) as usize;
    let decay = (-9.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut phase: f32 = 0.0;
    let mut noise_state: u64 = 0x1357_9BDF_2468_ACE0;
    for i in 0..n {
        let freq = sweep_freq(2400.0, 700.0, i, n);
        let amp = env_exp(i, attack, decay);
        let (noise, next) = lcg_noise(noise_state);
        noise_state = next;
        let sig = 0.7 * sine(phase) + 0.3 * noise;
        samples.push(sig * amp * 0.35);
        phase = advance_phase(phase, freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Oil flare — a fiery "fwoomp": rising filtered noise + a low rising body,
/// ~0.22 s (PYRO-on-OIL reaction).
fn synth_flare() -> Vec<u8> {
    let duration = 0.22_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.04) as usize;
    let decay = (-5.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut noise_state: u64 = 0x0FED_CBA9_8765_4321;
    let mut lp: f32 = 0.0;
    let mut body_phase: f32 = 0.0;
    for i in 0..n {
        let (raw, next) = lcg_noise(noise_state);
        noise_state = next;
        // Brighten as it builds (rising low-pass cutoff).
        let coeff = 0.05 + 0.30 * (i as f32 / n as f32);
        lp += coeff * (raw - lp);
        let amp = env_exp(i, attack, decay);
        let body_freq = sweep_freq(120.0, 320.0, i, n);
        let sig = 0.6 * lp + 0.4 * sine(body_phase);
        samples.push((sig * amp * 0.40).clamp(-1.0, 1.0));
        body_phase = advance_phase(body_phase, body_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Crit — a sharp bright "ting": a high sine + overtone, very short ~0.09 s.
fn synth_crit() -> Vec<u8> {
    let duration = 0.09_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.02) as usize;
    let decay = (-12.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut p1: f32 = 0.0;
    let mut p2: f32 = 0.0;
    for i in 0..n {
        let amp = env_exp(i, attack, decay);
        let sig = 0.6 * sine(p1) + 0.4 * sine(p2);
        samples.push(sig * amp * 0.32);
        p1 = advance_phase(p1, 1760.0);
        p2 = advance_phase(p2, 2640.0); // a bright overtone above
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Dash — a short airy "whoosh": filtered noise + a descending body tone, ~0.16 s.
/// Played when the player dashes (`skills::use_skills`).
fn synth_dash() -> Vec<u8> {
    let duration = 0.16_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.10) as usize;
    let decay = (-8.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut noise_state: u64 = 0x2468_ACE0_1357_9BDF;
    let mut lp: f32 = 0.0;
    let mut body_phase: f32 = 0.0;
    for i in 0..n {
        let (raw, next) = lcg_noise(noise_state);
        noise_state = next;
        // Darken as it fades (falling low-pass cutoff) — the air rushing past.
        let coeff = 0.35 - 0.25 * (i as f32 / n as f32);
        lp += coeff * (raw - lp);
        let amp = env_exp(i, attack, decay);
        let body_freq = sweep_freq(520.0, 160.0, i, n); // descending
        let sig = 0.7 * lp + 0.3 * sine(body_phase);
        samples.push((sig * amp * 0.30).clamp(-1.0, 1.0));
        body_phase = advance_phase(body_phase, body_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Power-weapon fire — a punchy "charged release": a descending body tone
/// (640→120 Hz) under a square-ish edge, heavier than the primary pew, ~0.20 s.
/// Played on any successful power-weapon shot (`power_weapon::fire_power_weapon`).
fn synth_power_fire() -> Vec<u8> {
    let duration = 0.20_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.02) as usize;
    let decay = (-6.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut body_phase: f32 = 0.0;
    let mut edge_phase: f32 = 0.0;
    for i in 0..n {
        let amp = env_exp(i, attack, decay);
        let body_freq = sweep_freq(640.0, 120.0, i, n);
        let edge_freq = body_freq * 1.5; // a fifth above for bite
        let sig = 0.7 * sine(body_phase) + 0.3 * square(edge_phase);
        samples.push((sig * amp * 0.42).clamp(-1.0, 1.0));
        body_phase = advance_phase(body_phase, body_freq);
        edge_phase = advance_phase(edge_phase, edge_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Bomb — a deep screen-clearing "whoomp + rumble": heavily low-passed noise
/// over a descending sub-bass body (160→38 Hz), loud + ~0.45 s. Played once when
/// the Bomb skill (X) detonates (`skills::use_skills`).
fn synth_bomb() -> Vec<u8> {
    let duration = 0.45_f32;
    let n = (SR * duration) as usize;
    let attack = (n as f32 * 0.01) as usize;
    let decay = (-4.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut noise_state: u64 = 0xBEEF_F00D_1234_5678;
    let mut lp: f32 = 0.0;
    let mut body_phase: f32 = 0.0;
    for i in 0..n {
        let (raw, next) = lcg_noise(noise_state);
        noise_state = next;
        lp += 0.08 * (raw - lp); // heavy low-pass → deep rumble
        let amp = env_exp(i, attack, decay);
        let body_freq = sweep_freq(160.0, 38.0, i, n);
        let sig = 0.5 * lp + 0.5 * sine(body_phase);
        samples.push((sig * amp * 0.60).clamp(-1.0, 1.0));
        body_phase = advance_phase(body_phase, body_freq);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

/// Level-up — a triumphant rising C-major arpeggio (C5→E5→G5→C6) with a soft
/// octave sparkle, ~0.4 s. Played when the account level increases mid-run.
fn synth_levelup() -> Vec<u8> {
    let duration = 0.40_f32;
    let n = (SR * duration) as usize;
    let notes = [523.25_f32, 659.25, 783.99, 1046.50];
    let seg_len = (n / notes.len()).max(1);
    let attack = (seg_len as f32 * 0.06) as usize;
    let decay = (-7.0_f32 / SR).exp();

    let mut samples = Vec::with_capacity(n);
    let mut phase: f32 = 0.0;
    for i in 0..n {
        let seg = (i / seg_len).min(notes.len() - 1);
        let amp = env_exp(i % seg_len, attack, decay); // re-attack each note
        let sig = 0.8 * sine(phase) + 0.2 * sine(2.0 * phase); // octave sparkle
        samples.push(sig * amp * 0.34);
        phase = advance_phase(phase, notes[seg]);
    }
    wav_pcm16_mono(&samples, SAMPLE_RATE)
}

// ── Resource ─────────────────────────────────────────────────────────────────

/// Throttle interval in seconds (mirrors `SOUND_THROTTLE_MS = 30`).
const THROTTLE_SECS: f64 = 0.030;

/// Master volume for file-based SFX.
const SFX_VOLUME: f32 = 0.8;

/// Holds synthesized fallback handles and all loaded WAV file handles.
///
/// `Sfx` is inserted at startup by `setup_sfx` and read by the playback
/// systems.  The public synth fields remain so that any code holding a
/// direct reference (e.g. future systems) still compiles.
#[derive(Resource)]
pub struct Sfx {
    // ── Synthesized fallback handles (kept for backward compat + fallback) ──
    pub player_shoot: Handle<AudioSource>,
    pub enemy_shoot:  Handle<AudioSource>,
    pub explosion:    Handle<AudioSource>,
    pub player_hit:   Handle<AudioSource>,
    pub pickup:       Handle<AudioSource>,
    pub shatter:      Handle<AudioSource>,
    pub flare:        Handle<AudioSource>,
    pub crit:         Handle<AudioSource>,
    pub levelup:      Handle<AudioSource>,
    pub dash:         Handle<AudioSource>,
    pub power_fire:   Handle<AudioSource>,
    pub bomb:         Handle<AudioSource>,

    // ── File-based SFX: event_key → variants ──
    /// Map from event key (e.g. `"shoot"`, `"enemyDestroy_HUNTER"`) to a
    /// list of `Handle<AudioSource>` loaded from `sfx/<key>_v<N>.wav` files
    /// (or the single `sfx/<key>.wav` when no variants exist).
    file_sfx: HashMap<String, Vec<Handle<AudioSource>>>,

    // ── Throttle state: event_key → last-played elapsed time (seconds) ──
    last_played: HashMap<String, f64>,

    // ── Pseudo-random variant counter ──
    variant_counter: u64,
}

impl Sfx {
    /// Choose a handle for `event`, applying the specific→generic fallback.
    ///
    /// Returns `None` when no file handle exists for this event (or its
    /// generic parent), signalling the caller to use the synth fallback.
    fn pick_file_handle(&mut self, event: &str) -> Option<Handle<AudioSource>> {
        // Try the specific event first; if missing, strip the last `_SUFFIX`.
        let handles = self
            .file_sfx
            .get(event)
            .or_else(|| {
                // Find the last underscore to strip one suffix level.
                let generic = event.rfind('_').map(|pos| &event[..pos])?;
                self.file_sfx.get(generic)
            })?;

        if handles.is_empty() {
            return None;
        }

        // Advance LCG counter and pick a variant index deterministically.
        self.variant_counter = self
            .variant_counter
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let idx = (self.variant_counter >> 33) as usize % handles.len();
        Some(handles[idx].clone())
    }

    /// Returns `true` when the event has been played within the throttle window.
    fn is_throttled(&self, event: &str, now_secs: f64) -> bool {
        self.last_played
            .get(event)
            .map(|&t| now_secs - t < THROTTLE_SECS)
            .unwrap_or(false)
    }

    /// Record that `event` was played at `now_secs`.
    fn record_played(&mut self, event: &str, now_secs: f64) {
        self.last_played.insert(event.to_string(), now_secs);
    }
}

// ── Setup system ─────────────────────────────────────────────────────────────

/// Synthesize fallback SFX, scan `sfx/` for WAV files, register all as
/// `AudioSource` assets, and insert the `Sfx` resource.  Wired into `Startup`.
pub fn setup_sfx(mut commands: Commands, mut assets: ResMut<Assets<AudioSource>>) {
    let make_synth = |bytes: Vec<u8>| AudioSource {
        bytes: Arc::from(bytes.as_slice()),
    };

    // ── Synthesized fallbacks ──
    let player_shoot = assets.add(make_synth(synth_player_shoot()));
    let enemy_shoot  = assets.add(make_synth(synth_enemy_shoot()));
    let explosion    = assets.add(make_synth(synth_explosion()));
    let player_hit   = assets.add(make_synth(synth_player_hit()));
    let pickup       = assets.add(make_synth(synth_pickup()));
    let shatter      = assets.add(make_synth(synth_shatter()));
    let flare        = assets.add(make_synth(synth_flare()));
    let crit         = assets.add(make_synth(synth_crit()));
    let levelup      = assets.add(make_synth(synth_levelup()));
    let dash         = assets.add(make_synth(synth_dash()));
    let power_fire   = assets.add(make_synth(synth_power_fire()));
    let bomb         = assets.add(make_synth(synth_bomb()));

    // ── Load WAV files from sfx/ ──
    let mut file_sfx: HashMap<String, Vec<Handle<AudioSource>>> = HashMap::new();

    let sfx_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/sfx");
    if let Ok(entries) = std::fs::read_dir(sfx_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            // Only process *.wav files.
            let Some(ext) = path.extension() else { continue };
            if !ext.eq_ignore_ascii_case("wav") {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            // Derive the event key by stripping the trailing `_v<digits>` suffix.
            // Files without that suffix (e.g. `shoot.wav` alongside `shoot_v1.wav`)
            // are treated as a single-entry variant group under the bare key.
            let event_key = strip_variant_suffix(stem);

            // Read the WAV bytes and register as an AudioSource asset.
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let handle = assets.add(AudioSource {
                bytes: Arc::from(bytes.as_slice()),
            });

            file_sfx
                .entry(event_key.to_string())
                .or_default()
                .push(handle);
        }
    }

    commands.insert_resource(Sfx {
        player_shoot,
        enemy_shoot,
        explosion,
        player_hit,
        pickup,
        shatter,
        flare,
        crit,
        levelup,
        dash,
        power_fire,
        bomb,
        file_sfx,
        last_played: HashMap::new(),
        variant_counter: 0x517C_C1B7_2722_0A95, // arbitrary non-zero seed
    });
}

/// Strip the trailing `_v<digits>` suffix from a WAV stem so that
/// `"shoot_v3"` → `"shoot"` and `"enemyDestroy_HUNTER_v2"` → `"enemyDestroy_HUNTER"`.
/// Stems without that suffix are returned unchanged.
fn strip_variant_suffix(stem: &str) -> &str {
    // Walk backwards: expect trailing digits, then 'v', then '_'.
    let bytes = stem.as_bytes();
    let mut i = bytes.len();

    // Consume trailing digits.
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    // Must be at least one digit.
    if i == bytes.len() {
        return stem;
    }
    // Next char must be 'v'.
    if i == 0 || bytes[i - 1] != b'v' {
        return stem;
    }
    i -= 1;
    // Next char must be '_'.
    if i == 0 || bytes[i - 1] != b'_' {
        return stem;
    }
    i -= 1;

    if i == 0 {
        stem // degenerate: the whole stem was `_v<N>`
    } else {
        &stem[..i]
    }
}

// ── Internal playback helper ─────────────────────────────────────────────────

/// Attempt to spawn an `AudioPlayer` for `event_key` from the loaded WAV files.
///
/// Returns `true` if a file-based sound was spawned, `false` if no file
/// exists for this event (caller should use the synth fallback).
///
/// Applies the 30 ms per-event throttle.  Uses specific→generic key fallback.
fn try_play_file(
    commands: &mut Commands,
    sfx: &mut Sfx,
    event_key: &str,
    now_secs: f64,
) -> bool {
    if sfx.is_throttled(event_key, now_secs) {
        // Throttled — report as "handled" so the synth is also suppressed.
        return true;
    }

    let Some(handle) = sfx.pick_file_handle(event_key) else {
        return false;
    };

    sfx.record_played(event_key, now_secs);

    commands.spawn((
        AudioPlayer::new(handle),
        PlaybackSettings {
            volume: Volume::Linear(SFX_VOLUME),
            ..PlaybackSettings::DESPAWN
        },
    ));
    true
}

// ── Playback systems ─────────────────────────────────────────────────────────

/// Spawn a shoot sound for every `Fire` message.
///
/// Player fires → `"shoot"` WAV (fallback: synth `player_shoot`).
/// Enemy fires  → synth `enemy_shoot` (no dedicated file yet).
/// Caps at 4 per faction per frame to prevent audio spam on rapid auto-fire.
pub fn play_shoot(
    mut commands: Commands,
    mut sfx: ResMut<Sfx>,
    mut fire: MessageReader<Fire>,
    time: Res<Time>,
) {
    let mut player_count = 0u32;
    let mut enemy_count = 0u32;
    const CAP: u32 = 4;
    let now = time.elapsed_secs_f64();

    for ev in fire.read() {
        match ev.faction {
            Faction::Player => {
                if player_count < CAP {
                    if !try_play_file(&mut commands, &mut sfx, "shoot", now) {
                        // Synth fallback
                        commands.spawn((
                            AudioPlayer::new(sfx.player_shoot.clone()),
                            PlaybackSettings::DESPAWN,
                        ));
                    }
                    player_count += 1;
                }
            }
            Faction::Enemy => {
                if enemy_count < CAP {
                    // No file key for generic enemy shoot; use synth directly.
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

/// Spawn an explosion/destroy sound for every `Death` message (capped at 6/frame).
///
/// Tries `"enemyDestroy"` WAV first; falls back to synth `explosion`.
pub fn play_explosion(
    mut commands: Commands,
    mut sfx: ResMut<Sfx>,
    mut deaths: MessageReader<Death>,
    time: Res<Time>,
) {
    const CAP: u32 = 6;
    let mut count = 0u32;
    let now = time.elapsed_secs_f64();

    for _ev in deaths.read() {
        if count < CAP {
            if !try_play_file(&mut commands, &mut sfx, "enemyDestroy", now) {
                // Synth fallback
                commands.spawn((
                    AudioPlayer::new(sfx.explosion.clone()),
                    PlaybackSettings::DESPAWN,
                ));
            }
            count += 1;
        }
    }
}

/// Play a pickup chime when the player collects an orb or powerup. Collapses a
/// same-frame cluster into one sound and applies the standard per-event throttle.
pub fn play_pickup(
    mut commands: Commands,
    mut sfx: ResMut<Sfx>,
    mut pickups: MessageReader<crate::messages::Pickup>,
    time: Res<Time>,
) {
    if pickups.read().count() == 0 {
        return;
    }
    let now = time.elapsed_secs_f64();
    if sfx.is_throttled("pickup", now) {
        return;
    }
    // Try a "pickup" WAV; otherwise the synth chime.
    if !try_play_file(&mut commands, &mut sfx, "pickup", now) {
        commands.spawn((AudioPlayer::new(sfx.pickup.clone()), PlaybackSettings::DESPAWN));
    }
    sfx.record_played("pickup", now);
}

/// Play a player-hit sound whenever the player entity receives `Damage`.
///
/// Tries `"playerHitEnemy"` WAV first, then `"hit"`, then synth fallback.
pub fn play_player_hit(
    mut commands: Commands,
    mut sfx: ResMut<Sfx>,
    mut dmg: MessageReader<Damage>,
    players: Query<(), With<crate::components::Ship>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();

    for ev in dmg.read() {
        if players.get(ev.target).is_ok() {
            // Try specific → generic → synth fallback.
            if !try_play_file(&mut commands, &mut sfx, "playerHitEnemy", now)
                && !try_play_file(&mut commands, &mut sfx, "hit", now)
            {
                commands.spawn((
                    AudioPlayer::new(sfx.player_hit.clone()),
                    PlaybackSettings::DESPAWN,
                ));
            }
        }
    }
}

/// Play an elemental-reaction sound for each `Reaction` (E4b): an icy crack for
/// SHATTER, a fiery fwoomp for FLARE. Capped per frame so a shatter chain doesn't
/// stack into a wall of sound.
pub fn play_reaction(
    mut commands: Commands,
    sfx: Res<Sfx>,
    mut reactions: MessageReader<Reaction>,
) {
    const CAP: u32 = 3;
    let mut count = 0u32;
    for r in reactions.read() {
        if count >= CAP {
            break;
        }
        let handle = match r.kind {
            ReactionFx::Shatter => sfx.shatter.clone(),
            ReactionFx::Flare => sfx.flare.clone(),
        };
        commands.spawn((AudioPlayer::new(handle), PlaybackSettings::DESPAWN));
        count += 1;
    }
}

/// Play one bright crit "ting" per frame in which any `Crit` fired (multiple
/// crits in a tick collapse to a single ding — avoids a machine-gun of pings).
pub fn play_crit(mut commands: Commands, sfx: Res<Sfx>, mut crits: MessageReader<Crit>) {
    if crits.read().count() > 0 {
        commands.spawn((AudioPlayer::new(sfx.crit.clone()), PlaybackSettings::DESPAWN));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every synth produces a non-empty, even-length PCM-16 WAV (44-byte header
    /// + samples) — a smoke test that the new reaction/crit synths are wired and
    /// don't panic.
    #[test]
    fn reaction_and_crit_synths_produce_wav_buffers() {
        for bytes in [synth_shatter(), synth_flare(), synth_crit()] {
            assert!(bytes.len() > 44, "WAV has a header + samples (got {})", bytes.len());
            assert_eq!(bytes.len() % 2, 0, "PCM-16 byte count is even");
            assert_eq!(&bytes[0..4], b"RIFF", "WAV RIFF magic");
        }
    }
}
