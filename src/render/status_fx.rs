//! Enemy status FX: a glowing ring around an afflicted enemy, one per active
//! status, each tinted to its element and at its own radius so stacked statuses
//! read as concentric rings. Pure presentation: spawned on `Added<…>`, each aura
//! is a *top-level* entity (so it ignores the enemy's boss-scale and is sized to
//! its collider), follows its target each frame, and despawns when the status —
//! or the target — is gone. It never touches sim state.

use crate::components::{
    Bleed, Burning, Chill, Collider, Conduct, Corrode, Frozen, Mark, Oil, Stunned,
};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// One aura per status verb. Burn/Stun are the originals; the rest are the
/// elemental statuses from the E3 status engine.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuraKind {
    Burn,
    Stun,
    Chill,
    Frozen,
    Conduct,
    Corrode,
    Mark,
    Oil,
    Bleed,
}

impl AuraKind {
    /// HDR-emissive ring tint (Bloom turns it into a glow). Themed to the
    /// status's element: pyro orange, cryo blue/white, volt yellow, toxic green/
    /// red, void purple, oil dark olive.
    fn color(self) -> Color {
        let (r, g, b) = match self {
            AuraKind::Burn => (9.0, 2.5, 0.3),    // pyro orange
            AuraKind::Stun => (0.5, 7.0, 9.0),    // cyan
            AuraKind::Chill => (2.0, 5.0, 9.0),   // light cryo blue
            AuraKind::Frozen => (5.0, 8.0, 9.0),  // icy white-blue
            AuraKind::Conduct => (9.0, 8.0, 1.0), // volt yellow
            AuraKind::Corrode => (2.0, 9.0, 1.0), // toxic green
            AuraKind::Mark => (7.0, 1.0, 9.0),    // void purple
            AuraKind::Oil => (1.2, 1.0, 0.4),     // dark olive (deliberately dim)
            AuraKind::Bleed => (9.0, 0.5, 1.0),   // deep red-magenta
        };
        Color::linear_rgb(r, g, b)
    }

    /// Collider-radius multiplier — distinct per kind so co-occurring statuses
    /// nest as concentric rings instead of overlapping.
    fn radius_mult(self) -> f32 {
        match self {
            AuraKind::Oil => 1.18,
            AuraKind::Burn => 1.25,
            AuraKind::Stun => 1.25,
            AuraKind::Chill => 1.32,
            AuraKind::Conduct => 1.38,
            AuraKind::Frozen => 1.45,
            AuraKind::Corrode => 1.52,
            AuraKind::Bleed => 1.58,
            AuraKind::Mark => 1.65,
        }
    }
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

/// Spawn an aura when an enemy newly gains a status (`Added<…>`). Re-inserting a
/// status (e.g. the Lance beam refreshing `Burning`, or a field re-applying
/// `Frozen`) does *not* re-fire `Added`, so a target keeps at most one aura per
/// kind.
#[allow(clippy::too_many_arguments)]
pub fn spawn_status_auras(
    mut commands: Commands,
    burning: Query<(Entity, &Collider), Added<Burning>>,
    stunned: Query<(Entity, &Collider), Added<Stunned>>,
    chill: Query<(Entity, &Collider), Added<Chill>>,
    frozen: Query<(Entity, &Collider), Added<Frozen>>,
    conduct: Query<(Entity, &Collider), Added<Conduct>>,
    corrode: Query<(Entity, &Collider), Added<Corrode>>,
    mark: Query<(Entity, &Collider), Added<Mark>>,
    oil: Query<(Entity, &Collider), Added<Oil>>,
    bleed: Query<(Entity, &Collider), Added<Bleed>>,
) {
    let mut spawn = |e: Entity, col: &Collider, kind: AuraKind| {
        commands.spawn((
            StatusAura { target: e, kind },
            aura_ring(col.radius * kind.radius_mult(), kind.color()),
            Transform::from_xyz(0.0, 0.0, AURA_Z),
        ));
    };
    for (e, col) in &burning {
        spawn(e, col, AuraKind::Burn);
    }
    for (e, col) in &stunned {
        spawn(e, col, AuraKind::Stun);
    }
    for (e, col) in &chill {
        spawn(e, col, AuraKind::Chill);
    }
    for (e, col) in &frozen {
        spawn(e, col, AuraKind::Frozen);
    }
    for (e, col) in &conduct {
        spawn(e, col, AuraKind::Conduct);
    }
    for (e, col) in &corrode {
        spawn(e, col, AuraKind::Corrode);
    }
    for (e, col) in &mark {
        spawn(e, col, AuraKind::Mark);
    }
    for (e, col) in &oil {
        spawn(e, col, AuraKind::Oil);
    }
    for (e, col) in &bleed {
        spawn(e, col, AuraKind::Bleed);
    }
}

/// Follow each aura's target and pulse/spin it; despawn when the status (or the
/// target) is gone. The `Without<StatusAura>` filter keeps the target read
/// disjoint from the aura's `&mut Transform` (no B0001).
#[allow(clippy::type_complexity)]
pub fn update_status_auras(
    time: Res<Time>,
    mut commands: Commands,
    targets: Query<
        (
            &Transform,
            Has<Burning>,
            Has<Stunned>,
            Has<Chill>,
            Has<Frozen>,
            Has<Conduct>,
            Has<Corrode>,
            Has<Mark>,
            Has<Oil>,
            Has<Bleed>,
        ),
        Without<StatusAura>,
    >,
    mut auras: Query<(Entity, &StatusAura, &mut Transform)>,
) {
    let t = time.elapsed_secs();
    for (ae, aura, mut atf) in &mut auras {
        let Ok((ttf, burning, stunned, chill, frozen, conduct, corrode, mark, oil, bleed)) =
            targets.get(aura.target)
        else {
            commands.entity(ae).despawn(); // target gone
            continue;
        };
        let active = match aura.kind {
            AuraKind::Burn => burning,
            AuraKind::Stun => stunned,
            AuraKind::Chill => chill,
            AuraKind::Frozen => frozen,
            AuraKind::Conduct => conduct,
            AuraKind::Corrode => corrode,
            AuraKind::Mark => mark,
            AuraKind::Oil => oil,
            AuraKind::Bleed => bleed,
        };
        if !active {
            commands.entity(ae).despawn();
            continue;
        }
        atf.translation = ttf.translation.truncate().extend(AURA_Z);
        match aura.kind {
            // Spinning reticles: stun + void mark.
            AuraKind::Stun => atf.rotation = Quat::from_rotation_z(t * 3.0),
            AuraKind::Mark => atf.rotation = Quat::from_rotation_z(-t * 2.0),
            // Fast electric/fire flicker.
            AuraKind::Burn => atf.scale = Vec3::splat(1.0 + 0.12 * (t * 9.0).sin()),
            AuraKind::Conduct => atf.scale = Vec3::splat(1.0 + 0.10 * (t * 16.0).sin()),
            // Everything else: a gentle slow pulse.
            _ => atf.scale = Vec3::splat(1.0 + 0.06 * (t * 4.0).sin()),
        }
    }
}
