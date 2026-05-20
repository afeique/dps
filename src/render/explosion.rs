//! GPU-compute explosion FX via `bevy_hanabi` (`docs/port-plan.md` §3.2 — the
//! payoff over the web build's 600-particle cap). One `EffectAsset` is built
//! once at startup; a one-shot burst is spawned at every `Death`. Colors are
//! HDR (> 1.0) so the camera's bloom turns the burst into a real flash.

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

use crate::messages::Death;

/// Handle to the shared explosion effect, built once at startup.
#[derive(Resource)]
pub struct ExplosionEffect(pub Handle<EffectAsset>);

/// Despawns a finished one-shot effect entity — hanabi does not auto-clean.
#[derive(Component)]
pub struct ExplosionTimer(Timer);

/// Build + register the explosion `EffectAsset` and stash its handle.
pub fn setup_explosion_effect(mut commands: Commands, mut effects: ResMut<Assets<EffectAsset>>) {
    let handle = effects.add(build_explosion_effect());
    commands.insert_resource(ExplosionEffect(handle));
}

fn build_explosion_effect() -> EffectAsset {
    let writer = ExprWriter::new();

    // Init: age 0, lifetime jittered 0.35–0.7 s.
    let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0_f32).expr());
    let lifetime = writer.lit(0.35_f32).uniform(writer.lit(0.7_f32)).expr();
    let init_lifetime = SetAttributeModifier::new(Attribute::LIFETIME, lifetime);

    // Init: start at a tiny scatter sphere, fly outward at 40–140 u/s.
    let init_pos = SetPositionSphereModifier {
        center: writer.lit(Vec3::ZERO).expr(),
        radius: writer.lit(2.0_f32).expr(),
        dimension: ShapeDimension::Volume,
    };
    let init_vel = SetVelocitySphereModifier {
        center: writer.lit(Vec3::ZERO).expr(),
        speed: writer.lit(40.0_f32).uniform(writer.lit(140.0_f32)).expr(),
    };

    // Update: drag so the burst decelerates and settles.
    let update_drag = LinearDragModifier::new(writer.lit(6.0_f32).expr());

    // Render: hot white-yellow → orange → dim red → transparent (HDR → bloom).
    // `bevy::prelude::Gradient` (UI) shadows hanabi's — qualify it.
    let mut color = bevy_hanabi::Gradient::new();
    color.add_key(0.0, Vec4::new(9.0, 7.0, 3.0, 1.0));
    color.add_key(0.2, Vec4::new(7.0, 2.5, 0.4, 1.0));
    color.add_key(0.6, Vec4::new(2.5, 0.5, 0.1, 0.7));
    color.add_key(1.0, Vec4::new(0.3, 0.0, 0.0, 0.0));

    // Render: shrink over life.
    let mut size = bevy_hanabi::Gradient::new();
    size.add_key(0.0, Vec3::splat(6.0));
    size.add_key(0.7, Vec3::splat(3.0));
    size.add_key(1.0, Vec3::ZERO);

    // One-shot burst of 120 particles (capacity has headroom).
    let spawner = SpawnerSettings::once(120.0_f32.into());

    EffectAsset::new(256, spawner, writer.finish())
        .with_name("explosion")
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_lifetime)
        .update(update_drag)
        .render(ColorOverLifetimeModifier::new(color))
        .render(SizeOverLifetimeModifier {
            gradient: size,
            screen_space_size: false,
        })
}

/// Spawn a one-shot explosion at each `Death` position (z above the play field).
pub fn spawn_on_death(
    mut commands: Commands,
    effect: Option<Res<ExplosionEffect>>,
    mut deaths: MessageReader<Death>,
) {
    let Some(effect) = effect else {
        deaths.clear(); // effect not registered yet — drop the events
        return;
    };
    for death in deaths.read() {
        commands.spawn((
            ParticleEffect::new(effect.0.clone()),
            Transform::from_translation(death.position.extend(2.0)),
            ExplosionTimer(Timer::from_seconds(1.2, TimerMode::Once)),
        ));
    }
}

/// Despawn explosion entities once their particles have faded.
pub fn tick_explosion_timers(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut ExplosionTimer)>,
) {
    for (e, mut timer) in &mut q {
        timer.0.tick(time.delta());
        if timer.0.is_finished() {
            commands.entity(e).despawn();
        }
    }
}
