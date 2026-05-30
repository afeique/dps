//! Muzzle flash (graphical parity with rainboids' `drawMuzzleFlash`). When a
//! *player* shot leaves the barrel, `emit_muzzle_flash` spawns a brief warm HDR
//! burst at the `Fire` origin, pointed along the shot direction; it shrinks over
//! its short `Lifetime` (`fade_muzzle_flash`) and despawns via `tick_lifetimes`.
//! Pure presentation — the flash carries no `Velocity`/`Collider`, so it just
//! sits at the nose and fades. Bloom flares the over-bright fill into a glow.

use crate::combat::element::Element;
use crate::components::{Faction, Lifetime};
use crate::messages::Fire;
use crate::systems::weapons::{CurrentWeapon, ElementInfusion};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// A muzzle-flash burst; holds its spawn lifetime so the fade can scale by it.
#[derive(Component)]
pub struct MuzzleFlash {
    pub max_life: f32,
}

/// Flash lifetime (s) — a quick pop, matching rainboids' ~6-frame timer.
const FLASH_LIFE: f32 = 0.07;

/// The muzzle-flash fill for the active firing `element`: the element's hue at
/// high brightness over a white-hot core bias, so it still reads as a hot flash
/// (and Bloom flares it) but lights up in the weapon's element — Kinetic stays a
/// cool white, an Elemental Infusion / Gravity Lance tints it. Pure + tested.
pub fn muzzle_color(element: Element) -> Color {
    let l = element.color().to_linear();
    Color::linear_rgb(l.red * 7.0 + 1.5, l.green * 7.0 + 1.5, l.blue * 7.0 + 1.5)
}

/// A forward-pointing burst (a kite elongated along local +Y = the shot
/// direction): bright tip ahead, short tail behind. HDR-bright so bloom glows it;
/// `color` tints it to the active element.
fn flash_shape(color: Color) -> Shape {
    let len = 15.0_f32; // forward reach
    let wide = 6.0_f32; // side half-width
    let tail = 5.0_f32; // short rear spike
    let path = ShapePath::new()
        .move_to(Vec2::new(0.0, len))
        .line_to(Vec2::new(wide, 0.0))
        .line_to(Vec2::new(0.0, -tail))
        .line_to(Vec2::new(-wide, 0.0))
        .close();
    ShapeBuilder::with(&path).fill(color).build()
}

/// Spawn a flash at the nose for each player shot fired this frame, tinted to the
/// active firing element (an infusion override wins over the weapon's base).
pub fn emit_muzzle_flash(
    mut commands: Commands,
    mut fired: MessageReader<Fire>,
    weapon: Res<CurrentWeapon>,
    infusion: Res<ElementInfusion>,
) {
    // Same for every shot this frame — resolve once.
    let element = infusion.element.unwrap_or_else(|| weapon.0.element());
    let color = muzzle_color(element);
    for f in fired.read() {
        if f.faction != Faction::Player {
            continue;
        }
        // Orient local +Y along the shot direction (Fire.dir is normalized).
        let angle = f.dir.to_angle() - std::f32::consts::FRAC_PI_2;
        commands.spawn((
            MuzzleFlash { max_life: FLASH_LIFE },
            flash_shape(color),
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
