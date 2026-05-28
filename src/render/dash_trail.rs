//! Dash afterimage trail (graphical parity with rainboids' dash ghosts). While
//! the ship is mid-dash — invulnerable *and* moving fast (the `ShiftLeft` dash
//! adds a +600 impulse; Shield Burst grants invuln but no speed, so it's
//! excluded) — `emit_dash_trail` stamps faint hull-silhouette ghosts at the
//! ship's pose. They shrink over their short `Lifetime` (`fade_dash_ghosts`) and
//! despawn via `tick_lifetimes`. Pure presentation; bloom softens the edge.

use crate::components::{Invulnerable, Lifetime, Ship, Velocity};
use crate::render::shapes;
use bevy::prelude::*;

/// A single dash afterimage; holds its spawn lifetime so the fade can scale by it.
#[derive(Component)]
pub struct DashGhost {
    pub max_life: f32,
}

/// Min speed (world u/s) to count as dashing — above normal flight, so only the
/// dash impulse (or a comparable burst) trails. Pairs with the invuln gate.
const DASH_GHOST_SPEED: f32 = 380.0;
/// Seconds between ghosts (caps the live count) and each ghost's lifetime.
const EMIT_INTERVAL: f32 = 0.03;
const GHOST_LIFE: f32 = 0.22;

/// Stamp a faint hull ghost at the ship's pose while it's dashing, throttled.
pub fn emit_dash_trail(
    time: Res<Time>,
    mut commands: Commands,
    mut accum: Local<f32>,
    ship: Query<(&Transform, &Velocity, Has<Invulnerable>), With<Ship>>,
) {
    let Ok((tf, vel, invuln)) = ship.single() else {
        return;
    };
    // Only during a dash: invulnerable AND moving faster than normal flight.
    if !invuln || vel.0.length() < DASH_GHOST_SPEED {
        *accum = 0.0;
        return;
    }
    *accum += time.delta_secs();
    if *accum < EMIT_INTERVAL {
        return;
    }
    *accum = 0.0;

    commands.spawn((
        DashGhost { max_life: GHOST_LIFE },
        shapes::ship_ghost(),
        // Match the ship's pose; sit just behind it so the live hull stays on top.
        Transform {
            translation: tf.translation.truncate().extend(-0.05),
            rotation: tf.rotation,
            ..default()
        },
        Lifetime { seconds: GHOST_LIFE },
    ));
}

/// Shrink each afterimage toward nothing as its (short) lifetime runs out.
pub fn fade_dash_ghosts(mut q: Query<(&DashGhost, &Lifetime, &mut Transform)>) {
    for (ghost, life, mut tf) in &mut q {
        let frac = (life.seconds / ghost.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac);
    }
}
