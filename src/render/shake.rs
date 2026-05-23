//! Camera screen shake (spec I.2) — Vlambeer-style juice.
//!
//! Purely additive *presentation*: impactful sim events (enemy deaths scaled by
//! boss tier / mini-boss, player hits) bump a `ScreenShake` magnitude, and
//! `apply_screen_shake` offsets the fixed camera by a decaying sin/cos + jitter
//! vector. It only nudges the camera `Transform` (which sits at the origin) and
//! decays back to zero, so it can never affect simulation or other systems.
//!
//! Spec I.2 offset: `sin/cos(time)*intensity*0.25 + random*intensity*0.75`,
//! magnitude kept as the stronger of stacked triggers, decremented per frame.

use crate::messages::{Death, PlayerHurt};
use bevy::prelude::*;

/// Current camera-shake magnitude (world px). Decays to 0; `apply_screen_shake`
/// turns it into a camera offset. `init_resource` in app.rs.
#[derive(Resource, Default)]
pub struct ScreenShake {
    pub intensity: f32,
}

impl ScreenShake {
    /// Trigger a shake, keeping the stronger of current/new (spec I.2: "only
    /// applied if stronger than current"), clamped to `SHAKE_MAX`.
    pub fn add(&mut self, magnitude: f32) {
        self.intensity = self.intensity.max(magnitude).min(SHAKE_MAX);
    }
}

/// Max shake magnitude (px) — clamps stacked triggers (boss-rage tantrum etc.).
const SHAKE_MAX: f32 = 26.0;
/// Decay (px/sec) ≈ one "unit" per 60 Hz frame → a big hit settles in ~0.4 s.
const SHAKE_DECAY: f32 = 60.0;
/// Oscillation frequency (rad/sec) of the sin/cos component.
const SHAKE_FREQ: f32 = 40.0;

/// Player-hit shake magnitude.
pub const HURT_SHAKE: f32 = 9.0;

/// Per-death shake magnitude: bosses shake hardest (≈ the spec rage shake of 22),
/// mini-bosses moderately, regular enemies a light pop.
pub fn death_shake(boss_tier: u8, mini_boss: bool) -> f32 {
    if boss_tier > 0 {
        18.0 + boss_tier as f32 * 2.0 // 20..26
    } else if mini_boss {
        12.0
    } else {
        6.0
    }
}

/// The Vlambeer shake offset (spec I.2). Jitter is a deterministic time-hash
/// (no RNG dependency in a render system). Each axis is bounded by `intensity`
/// (0.25 + 0.75 = 1.0).
pub fn shake_offset(intensity: f32, t: f32) -> Vec2 {
    if intensity <= 0.0 {
        return Vec2::ZERO;
    }
    let jx = (t * 91.7).sin() * (t * 13.3).cos(); // ~[-1, 1]
    let jy = (t * 57.1).cos() * (t * 23.9).sin();
    Vec2::new(
        (t * SHAKE_FREQ).sin() * intensity * 0.25 + jx * intensity * 0.75,
        (t * SHAKE_FREQ).cos() * intensity * 0.25 + jy * intensity * 0.75,
    )
}

/// Bump the shake on impactful events. Reads `Death` + `PlayerHurt` in `Update`,
/// alongside the explosion / damage-number FX that consume the same messages.
pub fn trigger_screen_shake(
    mut shake: ResMut<ScreenShake>,
    mut deaths: MessageReader<Death>,
    mut hurts: MessageReader<PlayerHurt>,
) {
    for d in deaths.read() {
        shake.add(death_shake(d.boss_tier, d.mini_boss));
    }
    for _ in hurts.read() {
        shake.add(HURT_SHAKE);
    }
}

/// Offset the fixed camera by the current shake, then decay (spec I.2).
/// Idempotent: at intensity 0 the camera sits at its base (origin); the shake
/// only touches x/y, never z.
pub fn apply_screen_shake(
    time: Res<Time>,
    mut shake: ResMut<ScreenShake>,
    mut cam: Query<&mut Transform, With<Camera2d>>,
) {
    let Ok(mut tf) = cam.single_mut() else {
        return;
    };
    let offset = shake_offset(shake.intensity, time.elapsed_secs());
    tf.translation.x = offset.x;
    tf.translation.y = offset.y;
    shake.intensity = (shake.intensity - SHAKE_DECAY * time.delta_secs()).max(0.0);
}
