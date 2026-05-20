//! Entity cleanup: expire `Lifetime`s and cull bullets that leave the play
//! area. Keeps entity counts bounded without manual pools (Bevy's archetype
//! storage is the pool — see `docs/port-plan.md` §1).

use crate::components::{Bullet, Lifetime};
use crate::resources::PlayBounds;
use bevy::prelude::*;

pub fn tick_lifetimes(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Lifetime)>) {
    let dt = time.delta_secs();
    for (e, mut life) in &mut q {
        life.seconds -= dt;
        if life.seconds <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

pub fn despawn_offscreen_bullets(
    mut commands: Commands,
    bounds: Res<PlayBounds>,
    q: Query<(Entity, &Transform), With<Bullet>>,
) {
    let margin = 48.0;
    for (e, tf) in &q {
        if tf.translation.x.abs() > bounds.half.x + margin
            || tf.translation.y.abs() > bounds.half.y + margin
        {
            commands.entity(e).despawn();
        }
    }
}
