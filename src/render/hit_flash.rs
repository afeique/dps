//! Player hit-flash (graphical parity with rainboids' `drawHitFlash`). When the
//! ship takes a hit (`PlayerHurt`), `emit_player_hit_flash` spawns a bright
//! white-cyan burst centred on the ship; it shrinks over its short `Lifetime`
//! (`fade_hit_flash`) and despawns via `tick_lifetimes`. This is the *localized*
//! "I got hit" read on the hull — complementary to the full-screen red
//! `ScreenFlash` (`render::flash::trigger_player_hurt_flash`). Pure presentation.

use crate::components::{Lifetime, Ship};
use crate::messages::PlayerHurt;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// A hit-flash burst; holds its spawn lifetime so the fade can scale by it.
#[derive(Component)]
pub struct HitFlash {
    pub max_life: f32,
}

/// Flash lifetime (s) — a quick pop, matching rainboids' ~8-frame timer.
const FLASH_LIFE: f32 = 0.13;
/// Burst radius (world u) ≈ 1.5× the ship silhouette.
const FLASH_RADIUS: f32 = 30.0;

/// A bright white-cyan filled disc, HDR-bright so bloom flares it into a halo.
fn flash_shape() -> Shape {
    let mut path = ShapePath::new();
    for i in 0..20 {
        let a = i as f32 / 20.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * FLASH_RADIUS, a.sin() * FLASH_RADIUS);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(7.0, 9.0, 9.5))
        .build()
}

/// Spawn one burst centred on the ship for any hit it took this frame.
pub fn emit_player_hit_flash(
    mut commands: Commands,
    mut hurts: MessageReader<PlayerHurt>,
    ship: Query<&Transform, With<Ship>>,
) {
    // Drain the reader fully; collapse multiple hits this frame into one burst.
    if hurts.read().count() == 0 {
        return;
    }
    let Ok(tf) = ship.single() else {
        return;
    };
    commands.spawn((
        HitFlash { max_life: FLASH_LIFE },
        flash_shape(),
        Transform::from_translation(tf.translation.truncate().extend(0.2)),
        Lifetime { seconds: FLASH_LIFE },
    ));
}

/// Shrink each burst toward nothing as its (short) lifetime runs out.
pub fn fade_hit_flash(mut q: Query<(&HitFlash, &Lifetime, &mut Transform)>) {
    for (flash, life, mut tf) in &mut q {
        let frac = (life.seconds / flash.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac);
    }
}
