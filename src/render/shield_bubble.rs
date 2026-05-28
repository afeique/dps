//! Invulnerability shield bubble (graphical parity with rainboids' shield
//! shimmer). A ring child of the ship is hidden in normal play and pulses into
//! view whenever the ship carries `Invulnerable` — Shield Burst (C), dash
//! i-frames, Second Wind, etc. — so the player can *see* their i-frames. Driven
//! by `update_shield_bubble` (a sibling of `charge_glow::update_charge_glow`);
//! pure presentation, bloom flares the over-bright ring.

use crate::components::{Invulnerable, Ship};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// The invuln shield-bubble ring (a child of the ship).
#[derive(Component)]
pub struct ShieldBubble;

/// Ring radius (world u) ≈ 1.6× the ship silhouette.
pub const BUBBLE_R: f32 = 35.0;

/// A bright cyan-white stroked ring — the bubble outline (bloom softens it).
pub fn bubble_ring() -> Shape {
    let mut path = ShapePath::new();
    for i in 0..32 {
        let a = i as f32 / 32.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * BUBBLE_R, a.sin() * BUBBLE_R);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .stroke((Color::linear_rgb(3.0, 7.5, 9.0), 2.0))
        .build()
}

/// Show + pulse the bubble while the ship is invulnerable; hide it otherwise.
pub fn update_shield_bubble(
    time: Res<Time>,
    ship: Query<Has<Invulnerable>, With<Ship>>,
    mut bubble: Query<&mut Transform, With<ShieldBubble>>,
) {
    let invuln = ship.single().unwrap_or(false);
    for mut tf in &mut bubble {
        let s = if invuln {
            1.0 + 0.06 * (time.elapsed_secs() * 10.0).sin()
        } else {
            0.0 // hidden during normal play
        };
        tf.scale = Vec3::splat(s);
    }
}
