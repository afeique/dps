//! Bullet-impact sparks (graphical parity with rainboids' on-hit particles).
//!
//! Two readers, one visual:
//! * `spawn_impact_sparks` reads `Damage` and, for each hit on a *non-player*
//!   target (enemy / asteroid / boss part), scatters an **element-colored** spark
//!   burst at the target — tinted by the target's damage element so a pyro kill
//!   throws orange sparks, a cryo hit icy-blue, etc.
//! * `spawn_hit_sparks` reads the `Hit` message — the explicit hit signal for the
//!   paths that bypass `Damage` (enemy bullets / rams chipping the **Core**), so
//!   those impacts spark too. `Hit` carries its own color + an impact normal, so
//!   the burst fans *back* along the incoming direction (a real ricochet) instead
//!   of a flat radial pop.
//!
//! Each shard drifts via `integrate` (carries `Velocity`, no `Collider`, so it
//! touches nothing), shrinks over its `Lifetime` (`fade_impact_sparks`), and
//! despawns via `tick_lifetimes`. Pure presentation. Sub-1 DoT ticks are skipped
//! so trails don't fizz.

use crate::components::{Lifetime, Ship, Velocity};
use crate::messages::{Damage, Hit};
use crate::systems::enemy::element_for;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// A single impact shard; holds its spawn lifetime so the fade can scale by it.
#[derive(Component)]
pub struct ImpactSpark {
    pub max_life: f32,
}

/// Shards per hit, their lifetime (s), and how fast they scatter (world u/s).
const SPARKS_PER_HIT: usize = 7;
const SPARK_LIFE: f32 = 0.22;
const SPARK_SPEED: f32 = 150.0;
/// Cone half-angle (rad) the burst fans into when it has an impact direction —
/// a tight backward spray off the surface, like a ricochet.
const SPARK_CONE: f32 = 0.7;

/// HDR-scale a color so Bloom flares the spark; keeps a warm-white floor so even
/// a dim element still pops bright at the impact point.
fn hdr_spark(c: Color, gain: f32) -> Color {
    let l = c.to_linear();
    Color::linear_rgb(
        (l.red * gain).max(2.0),
        (l.green * gain).max(1.6),
        (l.blue * gain).max(0.9),
    )
}

/// A tiny bright shard disc in `color` (HDR so bloom flares it).
fn spark_shape(color: Color) -> Shape {
    let r = 2.4_f32;
    let mut path = ShapePath::new();
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).fill(color).build()
}

/// Spawn one spark burst of `n` shards at `pos`, colored `color`. When `dir` is
/// non-zero the shards fan into a backward cone around it (ricochet); otherwise
/// they pop evenly outward. `base` rotates the fan so repeated hits don't stamp.
fn burst(
    commands: &mut Commands,
    pos: Vec2,
    color: Color,
    dir: Vec2,
    base: f32,
    n: usize,
    spread: f32,
) {
    let aimed = dir.length_squared() > 1e-6;
    let center_ang = if aimed { dir.y.atan2(dir.x) } else { 0.0 };
    for i in 0..n {
        let a = if aimed {
            // Fan within ±SPARK_CONE of the impact normal, jittered by `base`.
            center_ang + (i as f32 / (n.max(1) - 1).max(1) as f32 - 0.5) * 2.0 * SPARK_CONE
                + base * 0.3
        } else {
            base + i as f32 / n as f32 * std::f32::consts::TAU
        };
        let d = Vec2::new(a.cos(), a.sin());
        let speed = spread * (0.7 + 0.6 * ((i * 7 + 3) % 5) as f32 / 5.0);
        commands.spawn((
            ImpactSpark { max_life: SPARK_LIFE },
            spark_shape(color),
            Transform::from_translation((pos + d * 4.0).extend(0.16)),
            Velocity(d * speed),
            Lifetime { seconds: SPARK_LIFE },
        ));
    }
}

/// Scatter an element-colored burst from each enemy/asteroid hit this frame.
pub fn spawn_impact_sparks(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    targets: Query<(&Transform, Has<Ship>, Option<&crate::components::Enemy>)>,
    mut spin: Local<u32>,
) {
    for ev in dmg.read() {
        let Ok((tf, is_player, enemy)) = targets.get(ev.target) else {
            continue; // target gone this tick (a lethal hit) — explosion covers it
        };
        if is_player || ev.amount < 1.0 {
            continue; // player hits use the hit-flash; skip sub-1 DoT ticks
        }
        *spin = spin.wrapping_add(1);
        // Tint by the target's element so the spray reads its damage type.
        let color = match enemy {
            Some(e) => hdr_spark(element_for(e.kind).color(), 5.0),
            None => Color::linear_rgb(9.0, 8.0, 5.0), // asteroid / part — warm white
        };
        burst(
            &mut commands,
            tf.translation.truncate(),
            color,
            Vec2::ZERO,
            *spin as f32 * 0.7,
            SPARKS_PER_HIT,
            SPARK_SPEED,
        );
    }
}

/// Scatter a directional burst for every explicit `Hit` (the Core's hit paths).
pub fn spawn_hit_sparks(mut commands: Commands, mut hits: MessageReader<Hit>, mut spin: Local<u32>) {
    for ev in hits.read() {
        *spin = spin.wrapping_add(1);
        burst(
            &mut commands,
            ev.pos,
            hdr_spark(ev.color, 5.0),
            ev.dir,
            *spin as f32 * 0.7,
            SPARKS_PER_HIT,
            SPARK_SPEED,
        );
    }
}

/// Shrink each shard toward nothing as its (short) lifetime runs out.
pub fn fade_impact_sparks(mut q: Query<(&ImpactSpark, &Lifetime, &mut Transform)>) {
    for (spark, life, mut tf) in &mut q {
        let frac = (life.seconds / spark.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac);
    }
}
