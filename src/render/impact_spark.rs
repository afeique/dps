//! Bullet-impact sparks (graphical parity with rainboids' on-hit particles).
//! `spawn_impact_sparks` reads `Damage` events (its own cursor, like
//! `damage_numbers`), and for each hit on a *non-player* target scatters a few
//! short-lived bright shards outward from the target; they drift via `integrate`
//! (they carry `Velocity` but no `Collider`, so they touch nothing), shrink over
//! their `Lifetime` (`fade_impact_sparks`), and despawn via `tick_lifetimes`.
//! Pure presentation. Sub-1 DoT ticks are skipped so trails don't fizz.

use crate::components::{Lifetime, Ship, Velocity};
use crate::messages::Damage;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// A single impact shard; holds its spawn lifetime so the fade can scale by it.
#[derive(Component)]
pub struct ImpactSpark {
    pub max_life: f32,
}

/// Shards per hit, their lifetime (s), and how fast they scatter (world u/s).
const SPARKS_PER_HIT: usize = 5;
const SPARK_LIFE: f32 = 0.18;
const SPARK_SPEED: f32 = 130.0;

/// A tiny bright warm-white shard (HDR so bloom flares it).
fn spark_shape() -> Shape {
    let r = 2.4_f32;
    let mut path = ShapePath::new();
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(9.0, 8.0, 5.0))
        .build()
}

/// Scatter a burst of shards from each enemy/asteroid hit this frame.
pub fn spawn_impact_sparks(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    targets: Query<(&Transform, Has<Ship>)>,
    // Rotates each burst so repeated hits don't stamp the same star.
    mut spin: Local<u32>,
) {
    for ev in dmg.read() {
        let Ok((tf, is_player)) = targets.get(ev.target) else {
            continue; // target gone this tick (a lethal hit) — explosion covers it
        };
        if is_player || ev.amount < 1.0 {
            continue; // player hits use the hit-flash; skip sub-1 DoT ticks
        }
        *spin = spin.wrapping_add(1);
        let base = *spin as f32 * 0.7;
        let pos = tf.translation.truncate();
        for i in 0..SPARKS_PER_HIT {
            let a = base + i as f32 / SPARKS_PER_HIT as f32 * std::f32::consts::TAU;
            let dir = Vec2::new(a.cos(), a.sin());
            commands.spawn((
                ImpactSpark { max_life: SPARK_LIFE },
                spark_shape(),
                Transform::from_translation((pos + dir * 4.0).extend(0.15)),
                Velocity(dir * SPARK_SPEED),
                Lifetime { seconds: SPARK_LIFE },
            ));
        }
    }
}

/// Shrink each shard toward nothing as its (short) lifetime runs out.
pub fn fade_impact_sparks(mut q: Query<(&ImpactSpark, &Lifetime, &mut Transform)>) {
    for (spark, life, mut tf) in &mut q {
        let frac = (life.seconds / spark.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac);
    }
}
