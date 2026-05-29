//! Overdrive aura — a combat-readability cue for the Overdrive power-weapon buff
//! (a temporary primary-damage boost, `components::Overdrive`). A hot-gold ring
//! child of the ship pulses into view while the buff is active and hides
//! otherwise, so the player can *see* their shots are powered up. Mirrors
//! `shield_bubble`; pure presentation, bloom flares the over-bright ring.

use crate::components::{Overdrive, Ship};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// The Overdrive aura ring (a child of the ship).
#[derive(Component)]
pub struct OverdriveAura;

/// Ring radius (world u) — just outside the shield bubble so both read if stacked.
pub const AURA_R: f32 = 39.0;

/// A hot gold-orange stroked ring (distinct from the cyan shield bubble).
pub fn aura_ring() -> Shape {
    let mut path = ShapePath::new();
    for i in 0..32 {
        let a = i as f32 / 32.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * AURA_R, a.sin() * AURA_R);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .stroke((Color::linear_rgb(9.0, 5.0, 0.8), 2.0))
        .build()
}

/// Show + pulse the aura while the ship has the Overdrive buff; hide it otherwise.
pub fn update_overdrive_aura(
    time: Res<Time>,
    ship: Query<Has<Overdrive>, With<Ship>>,
    mut aura: Query<&mut Transform, With<OverdriveAura>>,
) {
    let active = ship.single().unwrap_or(false);
    for mut tf in &mut aura {
        let s = if active {
            1.0 + 0.08 * (time.elapsed_secs() * 12.0).sin()
        } else {
            0.0
        };
        tf.scale = Vec3::splat(s);
    }
}
