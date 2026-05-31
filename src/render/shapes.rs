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
/// The closed 16-vertex hull outline (Bevy space, nose +Y), shared by the solid
/// hull and the dash afterimage ghost so the silhouette stays single-sourced.
fn hull_path() -> ShapePath {
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
    path.close()
}

/// A cosmetic ship skin (graphical parity with rainboids' player skins): a named
/// emissive edge colour for the hull. The fill stays near-black; only the bloom-
/// flared stroke changes, so the silhouette reads the same in every skin.
#[derive(Clone, Copy)]
pub struct Skin {
    pub name: &'static str,
    /// HDR stroke colour (over-bright so `Bloom` flares it into the glow edge).
    pub edge: (f32, f32, f32),
}

/// The 12 selectable ship skins. Index 0 (Aurora cyan) is the default hull look.
pub const SKINS: [Skin; 12] = [
    Skin { name: "Aurora", edge: (0.2, 4.5, 7.5) },   // cyan (default)
    Skin { name: "Ember", edge: (7.5, 1.6, 0.3) },    // orange-red
    Skin { name: "Solar", edge: (8.0, 6.0, 0.6) },    // gold
    Skin { name: "Verdant", edge: (1.0, 7.0, 2.2) },  // green
    Skin { name: "Amethyst", edge: (5.0, 1.2, 8.0) }, // violet
    Skin { name: "Rose", edge: (8.0, 1.6, 4.5) },     // magenta-pink
    Skin { name: "Frost", edge: (4.5, 7.5, 8.5) },    // pale ice
    Skin { name: "Toxic", edge: (4.5, 8.0, 0.6) },    // acid green
    Skin { name: "Inferno", edge: (8.5, 3.0, 0.2) },  // hot orange
    Skin { name: "Abyss", edge: (1.2, 2.0, 8.5) },    // deep blue
    Skin { name: "Phantom", edge: (5.5, 5.5, 6.5) },  // silver-grey
    Skin { name: "Plasma", edge: (7.0, 0.6, 7.5) },   // electric purple
];

/// The skin at `index`, clamped to the [`SKINS`] table (out-of-range → default).
pub fn skin_for(index: usize) -> &'static Skin {
    SKINS.get(index).unwrap_or(&SKINS[0])
}

/// The hull rendered in a given [`Skin`]: near-black fill + the skin's emissive edge.
pub fn ship_hull_skin(skin: &Skin) -> Shape {
    let (r, g, b) = skin.edge;
    ShapeBuilder::with(&hull_path())
        .fill(Color::linear_rgb(0.0, 0.03, 0.08)) // near-black navy hull, stays dark
        .stroke((Color::linear_rgb(r, g, b), 2.0)) // emissive edge → bloom
        .build()
}

/// A faint cyan hull silhouette for the dash afterimage trail (`render::dash_trail`).
/// No dark fill — just a dim emissive edge that bloom softens into a ghost.
pub fn ship_ghost() -> Shape {
    ShapeBuilder::with(&hull_path())
        .stroke((Color::linear_rgb(0.15, 1.6, 2.6), 1.5))
        .build()
}

/// Bright cockpit highlight — spawned as a child of the hull (z above it).
/// JS cockpit sits at Canvas `(0, -r*0.42)` → Bevy `(0, +r*0.42)`.
/// The cockpit highlight colour for a skin: the skin's hue normalized to a bright
/// peak plus a white floor, so it reads as a hot highlight tinted toward the skin
/// (cyan-white for Aurora, warm-white for Ember, …). Returns linear RGB.
pub fn cockpit_rgb(skin: &Skin) -> (f32, f32, f32) {
    let (r, g, b) = skin.edge;
    let m = r.max(g).max(b).max(0.001);
    let s = 7.0 / m; // normalize the brightest channel to ~7
    (r * s + 2.0, g * s + 2.0, b * s + 2.0) // + white floor → stays a bright highlight
}

/// The cockpit highlight in a given [`Skin`]'s hue (see [`cockpit_rgb`]).
pub fn ship_cockpit_skin(skin: &Skin) -> Shape {
    const R: f32 = SHIP_R;
    let cockpit = shapes::Ellipse {
        radii: Vec2::new(R * 0.17, R * 0.21),
        center: Vec2::ZERO,
    };
    let (r, g, b) = cockpit_rgb(skin);
    ShapeBuilder::with(&cockpit)
        .fill(Color::linear_rgb(r, g, b)) // skin-tinted, blooms hot
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
