//! Enemy status FX: a glowing aura around an afflicted enemy — a flickering
//! orange ring while **Burning** (Lance), a spinning cyan ring while **Stunned**
//! (Arc / EMP / Stun Shot). Pure presentation: spawned on `Added<…>`, the aura is
//! a *top-level* entity (so it ignores the enemy's Transform scale and is sized to
//! its collider), follows its target each frame, and despawns when the status — or
//! the target — is gone. It never touches sim state.

use crate::components::{Burning, Collider, Stunned};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuraKind {
    Burn,
    Stun,
}

/// A status aura tracking `target`. Top-level (not a child) so the enemy's
/// boss-scale doesn't distort it.
#[derive(Component)]
pub struct StatusAura {
    pub target: Entity,
    pub kind: AuraKind,
}

/// Aura z — just above the enemy (z 0) so the ring reads around it.
const AURA_Z: f32 = 0.5;

/// A 28-gon ring stroke at `radius`, HDR-emissive so Bloom makes it glow.
fn aura_ring(radius: f32, color: Color) -> Shape {
    let mut path = ShapePath::new();
    for i in 0..28 {
        let a = i as f32 / 28.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).stroke((color, 2.0)).build()
}

/// Spawn an aura when an enemy newly gains a status (`Added<Burning/Stunned>`).
/// Re-inserting a status (the Lance beam refreshes Burning each tick) does *not*
/// re-fire `Added`, so a target keeps at most one aura per kind.
pub fn spawn_status_auras(
    mut commands: Commands,
    burning: Query<(Entity, &Collider), Added<Burning>>,
    stunned: Query<(Entity, &Collider), Added<Stunned>>,
) {
    for (e, col) in &burning {
        commands.spawn((
            StatusAura {
                target: e,
                kind: AuraKind::Burn,
            },
            aura_ring(col.radius * 1.25, Color::linear_rgb(9.0, 2.5, 0.3)),
            Transform::from_xyz(0.0, 0.0, AURA_Z),
        ));
    }
    for (e, col) in &stunned {
        commands.spawn((
            StatusAura {
                target: e,
                kind: AuraKind::Stun,
            },
            aura_ring(col.radius * 1.25, Color::linear_rgb(0.5, 7.0, 9.0)),
            Transform::from_xyz(0.0, 0.0, AURA_Z),
        ));
    }
}

/// Follow each aura's target and pulse/spin it; despawn when the status (or the
/// target) is gone. The `Without<StatusAura>` filter keeps the target read
/// disjoint from the aura's `&mut Transform` (no B0001).
pub fn update_status_auras(
    time: Res<Time>,
    mut commands: Commands,
    targets: Query<(&Transform, Has<Burning>, Has<Stunned>), Without<StatusAura>>,
    mut auras: Query<(Entity, &StatusAura, &mut Transform)>,
) {
    let t = time.elapsed_secs();
    for (ae, aura, mut atf) in &mut auras {
        let Ok((ttf, burning, stunned)) = targets.get(aura.target) else {
            commands.entity(ae).despawn(); // target gone
            continue;
        };
        let active = match aura.kind {
            AuraKind::Burn => burning,
            AuraKind::Stun => stunned,
        };
        if !active {
            commands.entity(ae).despawn();
            continue;
        }
        atf.translation = ttf.translation.truncate().extend(AURA_Z);
        match aura.kind {
            AuraKind::Burn => atf.scale = Vec3::splat(1.0 + 0.12 * (t * 9.0).sin()), // flicker
            AuraKind::Stun => atf.rotation = Quat::from_rotation_z(t * 3.0),         // spin
        }
    }
}
