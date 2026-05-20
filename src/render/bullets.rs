//! Shared bullet render assets. Bullet *spawning* (reading `Fire`) lives in
//! `systems::weapons`; this just caches one mesh + the per-team materials so
//! every shot reuses handles instead of allocating a mesh + material per
//! bullet (the old per-shot churn). Bright HDR-emissive colors → bloom glow.

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

/// Cached handles for the bullet layer, built once at startup.
#[derive(Resource)]
pub struct BulletAssets {
    /// Unit circle (radius 1); bullets scale it via `Transform`.
    pub circle: Handle<Mesh>,
    /// Player bullet body — bright yellow (`#FFFF00`).
    pub player_body: Handle<ColorMaterial>,
    /// Player bullet white-hot core (child of the body).
    pub player_core: Handle<ColorMaterial>,
    /// Enemy bullet body — hot magenta.
    pub enemy_body: Handle<ColorMaterial>,
    /// GPU particle trail attached to every player bullet.
    pub player_trail: Handle<EffectAsset>,
}

pub fn setup_bullet_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut effects: ResMut<Assets<EffectAsset>>,
) {
    commands.insert_resource(BulletAssets {
        circle: meshes.add(Circle::new(1.0)),
        player_body: materials.add(ColorMaterial::from(Color::linear_rgb(9.0, 8.0, 1.0))),
        player_core: materials.add(ColorMaterial::from(Color::linear_rgb(10.0, 10.0, 9.0))),
        enemy_body: materials.add(ColorMaterial::from(Color::linear_rgb(9.0, 0.7, 3.0))),
        player_trail: effects.add(build_player_trail()),
    });
}

fn build_player_trail() -> EffectAsset {
    let writer = ExprWriter::new();

    // Init: age 0, short lifetime — 0.15–0.3 s so the streak is tight.
    let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0_f32).expr());
    let lifetime = writer.lit(0.15_f32).uniform(writer.lit(0.3_f32)).expr();
    let init_lifetime = SetAttributeModifier::new(Attribute::LIFETIME, lifetime);

    // Init: emit from a hair-thin sphere so particles start at the bullet tip.
    let init_pos = SetPositionSphereModifier {
        center: writer.lit(Vec3::ZERO).expr(),
        radius: writer.lit(0.5_f32).expr(),
        dimension: ShapeDimension::Volume,
    };

    // Init: near-zero velocity — we want particles to hang in world space
    // behind the moving bullet, not fly outward.  A tiny spread (0–3 u/s) adds
    // a soft halo rather than a hard line.
    let init_vel = SetVelocitySphereModifier {
        center: writer.lit(Vec3::ZERO).expr(),
        speed: writer.lit(0.0_f32).uniform(writer.lit(3.0_f32)).expr(),
    };

    // Render: HDR yellow → transparent over life → bloom glow.
    // `bevy::prelude::Gradient` (UI) shadows hanabi's, so qualify it.
    let mut color = bevy_hanabi::Gradient::new();
    color.add_key(0.0, Vec4::new(8.0, 7.0, 1.5, 1.0));
    color.add_key(0.4, Vec4::new(6.0, 4.0, 0.5, 0.8));
    color.add_key(1.0, Vec4::new(4.0, 2.0, 0.0, 0.0));

    // Render: start small, shrink to nothing so the trail tapers cleanly.
    let mut size = bevy_hanabi::Gradient::new();
    size.add_key(0.0, Vec3::splat(2.0));
    size.add_key(1.0, Vec3::ZERO);

    // Continuous emission at ~200 particles/second.
    // SpawnerSettings::rate takes a CpuValue<f32>; f32::into() yields Single.
    let spawner = SpawnerSettings::rate(200.0_f32.into());

    // SimulationSpace::Global keeps emitted particles in world space so they
    // linger behind the moving bullet instead of riding with it.
    EffectAsset::new(256, spawner, writer.finish())
        .with_name("player_bullet_trail")
        .with_simulation_space(SimulationSpace::Global)
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_lifetime)
        .render(ColorOverLifetimeModifier::new(color))
        .render(SizeOverLifetimeModifier {
            gradient: size,
            screen_space_size: false,
        })
}
