//! Mouse crosshair (spec VIII.1 `cursor.js`, simplified): a cyan reticle that
//! tracks the world-space aim point while mouse-aim is active, hidden otherwise.
//! Presentation-only — reads `Intent.aim`, writes a Transform/Visibility.

use crate::components::{Intent, Ship};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// Marks the single crosshair entity.
#[derive(Component)]
pub struct Crosshair;

/// A cyan ring with four radial ticks (HDR-emissive so Bloom gives it a glow).
fn crosshair_shape() -> Shape {
    let r = 10.0_f32;
    let tick = 5.0_f32;
    let segs = 24;
    let mut path = ShapePath::new();
    for i in 0..=segs {
        let a = i as f32 / segs as f32 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    for &(dx, dy) in &[(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let inner = Vec2::new(dx * (r + 2.0), dy * (r + 2.0));
        let outer = Vec2::new(dx * (r + 2.0 + tick), dy * (r + 2.0 + tick));
        path = path.move_to(inner).line_to(outer);
    }
    ShapeBuilder::with(&path)
        .stroke((Color::linear_rgb(1.0, 7.0, 9.0), 1.5))
        .build()
}

/// Spawn the crosshair once (hidden until mouse-aim activates).
pub fn spawn_crosshair(mut commands: Commands) {
    commands.spawn((
        Crosshair,
        crosshair_shape(),
        Transform::from_xyz(0.0, 0.0, 5.0),
        Visibility::Hidden,
    ));
}

/// Track the aim point while mouse-aim is active; hide otherwise (cursor left
/// the window, no player, or a non-playing state where the ship is gone).
pub fn update_crosshair(
    player: Query<&Intent, With<Ship>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<Crosshair>>,
) {
    let Ok((mut tf, mut vis)) = q.single_mut() else {
        return;
    };
    match player.single() {
        Ok(intent) if intent.aim_active => {
            tf.translation.x = intent.aim.x;
            tf.translation.y = intent.aim.y;
            *vis = Visibility::Visible;
        }
        _ => *vis = Visibility::Hidden,
    }
}
