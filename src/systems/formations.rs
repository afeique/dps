//! Generic enemy formations (spec IV.6 / `enemy/formations.js`). A freshly-warped
//! group of ≥3 non-boss/non-mini-boss enemies can bind into a **coordinated plan**
//! — orbit / weave / flank / cross / figure8 — and lerp toward per-slot targets
//! around the player, overriding their individual AI for a few seconds. Ends after
//! `duration` or when survivors drop below half.
//!
//! Port notes / simplifications:
//! - The AI override is done by `update_formations` running **after** `integrate`
//!   and overwriting each member's position (lerp toward slot) + zeroing its
//!   velocity — so the per-kind AI's movement output is superseded without
//!   touching the 10 AI systems (their velocity write is clobbered).
//! - Feel is playtest-bound (radii/durations/speeds are the JS defaults); the
//!   slot math + chooser are the unit-tested core. Additive + revertable: a
//!   formation only *overrides* movement for its window, then releases members
//!   back to their AI.

use crate::components::{Core, Enemy, FormationMember, Ship, Velocity};
use crate::resources::GameRng;
use bevy::prelude::*;

const TAU: f32 = std::f32::consts::TAU;

/// The five coordinated movement patterns (spec IV.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormationKind {
    Orbit,
    Weave,
    Flank,
    Cross,
    Figure8,
}

/// Per-slot target world position for `kind` at elapsed time `t` (seconds),
/// around `player`. Ported 1:1 from `formations.js::targetForSlot`. Pure.
#[allow(clippy::too_many_arguments)]
pub fn slot_target(
    kind: FormationKind,
    slot: usize,
    n: usize,
    t: f32,
    player: Vec2,
    radius: f32,
    angular_speed: f32,
    phase_seed: f32,
) -> Vec2 {
    let i = slot as f32;
    let nf = n.max(1) as f32;
    match kind {
        FormationKind::Orbit => {
            let a = phase_seed + (i / nf) * TAU + t * angular_speed;
            player + Vec2::new(a.cos(), a.sin()) * radius
        }
        FormationKind::Weave => {
            // Staggered y-offsets + a sine x-sweep so members criss-cross.
            let spread = (i - (nf - 1.0) / 2.0) * 60.0;
            let sweep_x = (t * 0.7 + i * 0.4).cos() * 240.0;
            let sine_y = (t * 1.6 + i * 0.9).sin() * 80.0;
            player + Vec2::new(sweep_x, spread + sine_y)
        }
        FormationKind::Flank => {
            // Even slots flank from the left, odd from the right, on arcs.
            let side = if slot % 2 == 0 { -1.0 } else { 1.0 };
            let half = (slot / 2) as f32;
            let half_total = n.div_ceil(2).max(1) as f32;
            let tier = if half_total > 1.0 { half / (half_total - 1.0) } else { 0.5 };
            let r = radius + tier * 60.0;
            let sweep = (t * 0.45 + i).cos() * 0.5;
            let angle = if side > 0.0 { sweep } else { std::f32::consts::PI + sweep };
            player + Vec2::new(angle.cos() * r, angle.sin() * r * 0.7)
        }
        FormationKind::Cross => {
            // Fixed angle per member + an oscillating radius → converge/pass/diverge.
            let pair_angle = phase_seed + (i / nf) * TAU;
            let r = radius * (0.45 + 0.55 * (t * 0.9 + i).sin().abs());
            player + Vec2::new(pair_angle.cos() * r, pair_angle.sin() * r)
        }
        FormationKind::Figure8 => {
            let phase = phase_seed + (i / nf) * TAU + t * 0.7;
            player + Vec2::new(phase.sin() * radius, (phase * 2.0).sin() * radius * 0.5)
        }
    }
}

/// Chosen formation type + its run parameters (`pickFormation`).
#[derive(Clone, Copy, Debug)]
pub struct FormationParams {
    pub kind: FormationKind,
    pub radius: f32,
    pub duration: f32,
    pub angular_speed: f32,
    pub lerp: f32,
}

/// Heuristic chooser (`pickFormation`): needs ≥3; pool grows with member count
/// (+cross ≥4, +figure8 ≥5); radius/duration scale with the wave. `None` if too
/// small. Consumes RNG for the type pick + the randomized speed/lerp.
pub fn pick_formation(member_count: usize, wave: u64, rng: &mut GameRng) -> Option<FormationParams> {
    if member_count < 3 {
        return None;
    }
    let mut pool = vec![FormationKind::Orbit, FormationKind::Weave, FormationKind::Flank];
    if member_count >= 4 {
        pool.push(FormationKind::Cross);
    }
    if member_count >= 5 {
        pool.push(FormationKind::Figure8);
    }
    let idx = ((rng.next_f32() * pool.len() as f32) as usize).min(pool.len() - 1);
    let kind = pool[idx];
    let w = wave as f32;
    Some(FormationParams {
        kind,
        radius: 180.0 + 120.0_f32.min(w * 6.0),
        // JS ms (6000 + min(6000, wave*250)) → seconds.
        duration: 6.0 + 6.0_f32.min(w * 0.25),
        angular_speed: 0.45 + rng.next_f32() * 0.4,
        lerp: 0.07 + rng.next_f32() * 0.05,
    })
}

/// One active formation: its plan + bound members + elapsed/lifetime.
pub struct Formation {
    pub kind: FormationKind,
    pub members: Vec<Entity>,
    pub initial_count: usize,
    pub elapsed: f32,
    pub duration: f32,
    pub radius: f32,
    pub angular_speed: f32,
    pub lerp: f32,
    pub phase_seed: f32,
}

/// All active formations (run-scoped). `update_formations` ticks + expires them.
#[derive(Resource, Default)]
pub struct Formations {
    pub active: Vec<Formation>,
}

/// Bind `members` into a new formation with `params`. Tags each with
/// `FormationMember` (so other systems can tell, and to gate re-binding).
pub fn create_formation(
    commands: &mut Commands,
    formations: &mut Formations,
    members: Vec<Entity>,
    params: FormationParams,
    rng: &mut GameRng,
) {
    if members.len() < 3 {
        return;
    }
    for &e in &members {
        commands.entity(e).insert(FormationMember);
    }
    formations.active.push(Formation {
        kind: params.kind,
        initial_count: members.len(),
        members,
        elapsed: 0.0,
        duration: params.duration,
        radius: params.radius,
        angular_speed: params.angular_speed,
        lerp: params.lerp,
        phase_seed: rng.next_f32() * TAU,
    });
}

/// Tick every formation (FixedUpdate, after `integrate`): drop dead members,
/// expire past `duration` or below half survivors (releasing members back to AI),
/// and lerp survivors toward their slot targets + face the motion. Overwriting
/// position + zeroing velocity overrides the per-kind AI for the window.
pub fn update_formations(
    time: Res<Time>,
    mut commands: Commands,
    mut formations: ResMut<Formations>,
    player: Query<&Transform, With<Core>>,
    // `Without<Core>` keeps the mut-`Transform` members disjoint from the Core
    // read above (the orbit centre); `Without<Ship>` excludes the commander.
    mut members: Query<
        (&mut Transform, &mut Velocity),
        (With<Enemy>, Without<Ship>, Without<Core>),
    >,
) {
    let dt = time.delta_secs();
    let Ok(ptf) = player.single() else {
        return;
    };
    let player_pos = ptf.translation.truncate();

    formations.active.retain_mut(|f| {
        f.elapsed += dt;
        // Drop despawned members.
        f.members.retain(|&e| members.contains(e));
        let n = f.members.len();
        let expired = f.elapsed > f.duration
            || n == 0
            || (n as f32) < (f.initial_count as f32) * 0.5;
        if expired {
            for &e in &f.members {
                if members.contains(e) {
                    commands.entity(e).remove::<FormationMember>();
                }
            }
            return false;
        }
        let t = f.elapsed;
        for (slot, &e) in f.members.iter().enumerate() {
            if let Ok((mut tf, mut vel)) = members.get_mut(e) {
                let target =
                    slot_target(f.kind, slot, n, t, player_pos, f.radius, f.angular_speed, f.phase_seed);
                let pos = tf.translation.truncate();
                let new = pos + (target - pos) * f.lerp;
                tf.translation.x = new.x;
                tf.translation.y = new.y;
                // Face the formation's motion (finite-difference probe; forward is +Y).
                let next = slot_target(
                    f.kind, slot, n, t + 0.05, player_pos, f.radius, f.angular_speed, f.phase_seed,
                );
                let dir = next - target;
                if dir.length_squared() > 1e-6 {
                    tf.rotation =
                        Quat::from_rotation_z(dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2);
                }
                vel.0 = Vec2::ZERO; // override the individual AI's movement
            }
        }
        true
    });
}

/// Release all formations (run reset) — members are despawned on a fresh run, so
/// just dropping the list suffices.
pub fn clear_formations(mut formations: ResMut<Formations>) {
    formations.active.clear();
}
