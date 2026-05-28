//! Muzzle flash (graphical parity with rainboids' `drawMuzzleFlash`). When a
//! *player* shot leaves the barrel, `emit_muzzle_flash` spawns a brief warm HDR
//! burst at the `Fire` origin, pointed along the shot direction; it shrinks over
//! its short `Lifetime` (`fade_muzzle_flash`) and despawns via `tick_lifetimes`.
//! Pure presentation — the flash carries no `Velocity`/`Collider`, so it just
//! sits at the nose and fades. Bloom flares the over-bright fill into a glow.

use crate::components::{Faction, Lifetime};
use crate::messages::Fire;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// A muzzle-flash burst; holds its spawn lifetime so the fade can scale by it.
#[derive(Component)]
pub struct MuzzleFlash {
    pub max_life: f32,
}

/// Flash lifetime (s) — a quick pop, matching rainboids' ~6-frame timer.
const FLASH_LIFE: f32 = 0.07;

/// A warm forward-pointing burst (a kite elongated along local +Y = the shot
/// direction): bright tip ahead, short tail behind. HDR-bright so bloom glows it.
fn flash_shape() -> Shape {
    let len = 15.0_f32; // forward reach
    let wide = 6.0_f32; // side half-width
    let tail = 5.0_f32; // short rear spike
    let path = ShapePath::new()
        .move_to(Vec2::new(0.0, len))
        .line_to(Vec2::new(wide, 0.0))
        .line_to(Vec2::new(0.0, -tail))
        .line_to(Vec2::new(-wide, 0.0))
        .close();
    // Warm yellow-white (~rainboids '255, 220, 140'), pushed bright for bloom.
    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(9.0, 7.0, 3.2))
        .build()
}

/// Spawn a flash at the nose for each player shot fired this frame.
pub fn emit_muzzle_flash(mut commands: Commands, mut fired: MessageReader<Fire>) {
    for f in fired.read() {
        if f.faction != Faction::Player {
            continue;
        }
        // Orient local +Y along the shot direction (Fire.dir is normalized).
        let angle = f.dir.to_angle() - std::f32::consts::FRAC_PI_2;
        commands.spawn((
            MuzzleFlash { max_life: FLASH_LIFE },
            flash_shape(),
            Transform {
                translation: f.origin.extend(0.2), // just above bullets
                rotation: Quat::from_rotation_z(angle),
                ..default()
            },
            Lifetime { seconds: FLASH_LIFE },
        ));
    }
}

/// Shrink each flash toward nothing as its (short) lifetime runs out.
pub fn fade_muzzle_flash(mut q: Query<(&MuzzleFlash, &Lifetime, &mut Transform)>) {
    for (flash, life, mut tf) in &mut q {
        let frac = (life.seconds / flash.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac);
    }
}
