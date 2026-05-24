//! Boss-rage telegraph ring pulse (spec IV.7 polish). The warning ring spawned
//! by `systems::enemy::boss_rage` is a static lyon stroke with a
//! `Lifetime == TELEGRAPH_SECS`; this presentation system makes it **throb** —
//! growing outward as the rage charges, with a fast overlaid pulse — so the
//! counterplay beat reads at a glance. Pure presentation (scales the ring's
//! Transform); the sim/timer is untouched.

use crate::components::{Lifetime, TelegraphRing};
use crate::systems::enemy::TELEGRAPH_SECS;
use bevy::prelude::*;

/// Scale for a telegraph ring at `elapsed_frac` (0 = just spawned, 1 = about to
/// rage): a steady grow toward activation plus a 3-cycle throb. Pure + tested.
pub fn telegraph_pulse_scale(elapsed_frac: f32) -> f32 {
    let f = elapsed_frac.clamp(0.0, 1.0);
    let grow = 1.0 + 0.22 * f; // swells as the rage charges
    let throb = 0.07 * (f * std::f32::consts::TAU * 3.0).sin(); // fast flicker
    grow + throb
}

/// Pulse every live telegraph ring by scaling its Transform from the remaining
/// `Lifetime` (which `tick_lifetimes` counts down over `TELEGRAPH_SECS`).
pub fn pulse_telegraph_rings(mut q: Query<(&mut Transform, &Lifetime), With<TelegraphRing>>) {
    for (mut tf, life) in &mut q {
        let elapsed_frac = 1.0 - (life.seconds / TELEGRAPH_SECS).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(telegraph_pulse_scale(elapsed_frac));
    }
}
