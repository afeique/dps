//! Dark Prism Solid (`dps`) — Phase 0 graphics + toolchain spike.
//!
//! Proves the Rust + Bevy stack and the core graphics direction before the
//! gameplay port begins: an HDR `Camera2d` with bloom, plus a couple of bright
//! emissive silhouettes drifting around so the glow is visible in motion.
//!
//! Deliberately minimal. The real port — ECS gameplay, lyon-tessellated
//! ship/enemy silhouettes ported from `js/modules/render/shapes.js`,
//! `bevy_hanabi` GPU-compute particles, audio, and UI — is staged in
//! `docs/Rust + Bevy Port Plan – 2026-05-20.md`.

use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Dark Prism Solid — Phase 0 spike".into(),
                ..default()
            }),
            ..default()
        }))
        // Near-black backdrop so the bloom reads strongly.
        .insert_resource(ClearColor(Color::srgb(0.015, 0.01, 0.03)))
        .add_systems(Startup, setup)
        .add_systems(Update, drift)
        .run();
}

/// Linear + angular drift so the glow is visible in motion.
#[derive(Component)]
struct Drift {
    vel: Vec2,
    spin: f32,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // HDR 2D camera with bloom — the foundation of the "best graphics" goal.
    // Real bloom replaces the web build's per-canvas `shadowBlur` fakery.
    commands.spawn((
        Camera2d,
        Camera {
            hdr: true,
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::default(),
    ));

    // Over-bright (>1.0) emissive colors so Bloom produces a real glow halo.
    let prism_violet = materials.add(ColorMaterial::from(Color::linear_rgb(5.0, 1.2, 9.0)));
    let prism_cyan = materials.add(ColorMaterial::from(Color::linear_rgb(0.6, 7.0, 8.0)));

    // Stand-in silhouettes: a triangle "ship" + a hexagon "enemy". These are
    // replaced by lyon-tessellated ports of `shapes.js` in Phase 2.
    let ship = meshes.add(RegularPolygon::new(36.0, 3));
    let enemy = meshes.add(RegularPolygon::new(30.0, 6));

    commands.spawn((
        Mesh2d(ship),
        MeshMaterial2d(prism_violet),
        Transform::from_xyz(-180.0, -40.0, 0.0),
        Drift {
            vel: Vec2::new(95.0, 55.0),
            spin: 1.4,
        },
    ));

    commands.spawn((
        Mesh2d(enemy),
        MeshMaterial2d(prism_cyan),
        Transform::from_xyz(160.0, 60.0, 0.0),
        Drift {
            vel: Vec2::new(-70.0, -85.0),
            spin: -0.9,
        },
    ));
}

fn drift(time: Res<Time>, mut q: Query<(&mut Transform, &mut Drift)>) {
    let dt = time.delta_secs();
    for (mut t, mut d) in &mut q {
        t.translation.x += d.vel.x * dt;
        t.translation.y += d.vel.y * dt;
        t.rotate_z(d.spin * dt);
        // Bounce inside a rough play box.
        if t.translation.x.abs() > 320.0 {
            d.vel.x = -d.vel.x;
        }
        if t.translation.y.abs() > 220.0 {
            d.vel.y = -d.vel.y;
        }
    }
}
