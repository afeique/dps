//! Enemy spawn-in "warp" — port of rainboids' `warping` materialize. A freshly
//! spawned wave enemy carries [`WarpIn`] (attached in `enemy::spawn_enemy`);
//! [`flash_warp_in`] pops a bright cyan warp ring at its arrival point the moment
//! the marker appears, and [`tick_warp_in`] grows its silhouette from `0.3×` up to
//! its true scale over `dur`, then drops the marker. **Visual only** — the enemy's
//! Collider is full-size the whole time, so engagement timing is identical to a
//! plain pop-in. The warp ring reuses the reaction `Shockwave` machinery.

use crate::components::WarpIn;
use crate::render::reaction_fx::{Shockwave, unit_ring};
use bevy::prelude::*;

/// Pop a warp ring the moment an enemy begins materializing (`Added<WarpIn>`).
/// The ring scales with the enemy's size so a boss warps in with a bigger flash.
pub fn flash_warp_in(mut commands: Commands, q: Query<(&Transform, &WarpIn), Added<WarpIn>>) {
    for (tf, w) in &q {
        let peak = (28.0 * w.scale).clamp(22.0, 90.0);
        commands.spawn((
            Shockwave { age: 0.0, peak },
            unit_ring(Color::linear_rgb(3.0, 8.0, 9.0)), // bright warp cyan
            Transform::from_translation(tf.translation.truncate().extend(0.25))
                .with_scale(Vec3::splat(1.0)),
        ));
    }
}

/// Grow each materializing enemy from `0.3×` to its true scale (ease-out), then
/// snap to the exact scale and drop the marker so nothing keeps touching it.
pub fn tick_warp_in(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut WarpIn, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut w, mut tf) in &mut q {
        w.elapsed += dt;
        let t = (w.elapsed / w.dur).clamp(0.0, 1.0);
        if t >= 1.0 {
            tf.scale = Vec3::splat(w.scale);
            commands.entity(e).remove::<WarpIn>();
        } else {
            let ease = 1.0 - (1.0 - t) * (1.0 - t); // ease-out
            tf.scale = Vec3::splat(w.scale * (0.3 + 0.7 * ease));
        }
    }
}
