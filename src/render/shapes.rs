//! Lyon-tessellated silhouettes ported from the JS Canvas2D `shapes.js`
//! (`docs/port-plan.md` §3.1). Canvas2D is +Y-down with shapes authored
//! centered on the origin; Bevy 2D is +Y-up, so every Y is negated on the way
//! in. Glow is *not* per-shape blur — these use over-bright HDR-emissive
//! fill/stroke colors that the camera's `Bloom` turns into halos.

use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

/// Player ship authored radius (world units). Bumped from the JS `r = 15` so
/// the hero element reads larger than the enemies at the native window scale.
pub const SHIP_R: f32 = 22.0;

/// The player ship hull: the full 16-vertex silhouette from
/// `player/renderer.js` (central hull + wings + wing tips), authored at
/// `r = 15` and flipped into Bevy space so the nose points +Y — the ship's
/// forward vector (`tf.rotation * Vec3::Y`).
pub fn ship_hull() -> Shape {
    const R: f32 = SHIP_R;
    // (x, y) in Canvas space (+Y down); Y is negated when fed to lyon below.
    let pts = [
        (0.0, -R),
        (R * 0.32, -R * 0.18),
        (R * 1.12, R * 0.28),
        (R * 1.42, R * 0.08),
        (R * 1.18, R * 0.56),
        (R * 0.82, R * 0.68),
        (R * 0.42, R * 0.78),
        (R * 0.28, R * 0.58),
        (0.0, R * 0.38),
        (-R * 0.28, R * 0.58),
        (-R * 0.42, R * 0.78),
        (-R * 0.82, R * 0.68),
        (-R * 1.18, R * 0.56),
        (-R * 1.42, R * 0.08),
        (-R * 1.12, R * 0.28),
        (-R * 0.32, -R * 0.18),
    ];

    let mut path = ShapePath::new().move_to(Vec2::new(pts[0].0, -pts[0].1));
    for &(x, y) in &pts[1..] {
        path = path.line_to(Vec2::new(x, -y));
    }
    let path = path.close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.0, 0.03, 0.08)) // near-black navy hull, stays dark
        .stroke((Color::linear_rgb(0.2, 4.5, 7.5), 2.0)) // emissive cyan edge → bloom
        .build()
}

/// Bright cockpit highlight — spawned as a child of the hull (z above it).
/// JS cockpit sits at Canvas `(0, -r*0.42)` → Bevy `(0, +r*0.42)`.
pub fn ship_cockpit() -> Shape {
    const R: f32 = SHIP_R;
    let cockpit = shapes::Ellipse {
        radii: Vec2::new(R * 0.17, R * 0.21),
        center: Vec2::ZERO,
    };
    ShapeBuilder::with(&cockpit)
        .fill(Color::linear_rgb(3.0, 7.0, 9.0)) // cyan-white, blooms hot
        .build()
}

/// Local-space offset of the cockpit child relative to the hull (Bevy space).
pub const SHIP_COCKPIT_OFFSET: Vec2 = Vec2::new(0.0, SHIP_R * 0.42);

/// The Drifter: a 10-point electric star (`render/shapes.js`
/// `drawEnemyDrifterShape`) with alternating outer/inner radii, a near-black
/// body and an electric-cyan emissive edge. Radially symmetric, so the Y flip
/// is a no-op here.
pub fn drifter_star(radius: f32) -> Shape {
    let outer = radius * 0.88;
    let inner = radius * 0.48;

    let mut path = ShapePath::new();
    for i in 0..10 {
        let angle = (i as f32 / 10.0) * TAU;
        let r = if i % 2 == 0 { outer } else { inner };
        let p = Vec2::new(angle.cos() * r, angle.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    let path = path.close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.0, 0.015, 0.03)) // #000a10 body
        .stroke((Color::linear_rgb(0.0, 6.0, 8.0), 2.0)) // electric cyan edge
        .build()
}

/// The Drifter's white-hot core (child of the star, z above it).
pub fn drifter_core(radius: f32) -> Shape {
    let core = shapes::Circle {
        radius: radius * 0.32,
        center: Vec2::ZERO,
    };
    ShapeBuilder::with(&core)
        .fill(Color::linear_rgb(8.0, 9.0, 9.0)) // white-cyan core, blooms
        .build()
}
