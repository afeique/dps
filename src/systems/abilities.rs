//! The 4-slot ability loadout activation (Phase AB). Ticks the per-slot
//! cooldowns and, on an ability-key press for a ready slot holding an equipped
//! ability, fires that ability's effect. Reuses dps's existing effect code
//! (Bulwark/Field Medic/Deflector Orbs/EMP Pulse) and adds Blink.
//!
//! **Key binding (transitional):** the slots fire on **Numpad 1–4** for now,
//! because Digit 1–5 already select weapons (`systems::weapons::cycle_weapon`).
//! The final binding (Digit 1–4, with weapon-select moved into the radial menu)
//! lands with the BUILD-tree / input rework in Phase UI. The legacy keybinds
//! (C/X/V/G/H/J/K) still work in parallel during the migration.

use crate::components::*;
use crate::systems::skills::{spawn_deflector_orbs, EMP_RADIUS};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

/// Numpad keys 1–4 → loadout slots 0–3.
const SLOT_KEYS: [KeyCode; 4] = [
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
];

/// Blink teleport distance (px) and i-frame (s) — `player/abilities.js` Blink.
const BLINK_DISTANCE: f32 = 220.0;
const BLINK_IFRAME: f32 = 0.35;

/// Tick the loadout cooldowns and fire equipped abilities on their slot key.
/// A press is consumed (cooldown spent) only if the slot holds an *implemented*
/// ability that is off cooldown; empty/on-cooldown/deferred slots are no-ops.
pub fn activate_loadout(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut commands: Commands,
    equipped: Res<EquippedAbilities>,
    mut cds: ResMut<AbilityCooldowns>,
    mut player: Query<(Entity, &mut Transform), With<Ship>>,
    enemies: Query<(Entity, &Transform), (With<Enemy>, Without<Ship>)>,
) {
    cds.tick(time.delta_secs());

    let Ok((player_e, mut ptf)) = player.single_mut() else {
        return; // no ship (GameOver, etc.) — cooldowns still ticked above
    };
    let center = ptf.translation.truncate();
    let facing = (ptf.rotation * Vec3::Y).truncate().normalize_or_zero();

    for slot in 0..4 {
        if !keys.just_pressed(SLOT_KEYS[slot]) {
            continue;
        }
        let Some(ability) = equipped.0[slot] else {
            continue; // empty slot
        };
        if !cds.is_ready(slot) {
            continue; // still cooling down
        }

        let fired = match ability {
            Ability::Bulwark => {
                commands.entity(player_e).insert(Bulwark { seconds: 4.0 });
                true
            }
            Ability::FieldMedic => {
                commands.entity(player_e).insert(Repairing {
                    seconds: 5.0,
                    rate: 3.0,
                });
                true
            }
            Ability::DeflectorOrbs => {
                spawn_deflector_orbs(&mut commands, center);
                true
            }
            Ability::EmpPulse => {
                for (e, etf) in &enemies {
                    if etf.translation.truncate().distance(center) <= EMP_RADIUS {
                        commands.entity(e).insert(Stunned { secs: 2.0 });
                    }
                }
                true
            }
            Ability::Blink => {
                let dest = center + facing * BLINK_DISTANCE;
                ptf.translation.x = dest.x;
                ptf.translation.y = dest.y;
                commands
                    .entity(player_e)
                    .insert(Invulnerable { seconds: BLINK_IFRAME });
                true
            }
            Ability::CryoField
            | Ability::StasisField
            | Ability::StormCell
            | Ability::PyreAura => {
                if let Some((status, radius, tick)) = ability.field_params() {
                    spawn_ability_field(
                        &mut commands,
                        center,
                        status,
                        radius,
                        ability.duration(),
                        tick,
                    );
                }
                true
            }
            // Deferred abilities (effects land in later increments).
            _ => false,
        };

        if fired {
            cds.trigger(slot, ability.cooldown());
        }
    }
}

/// Spawn a drop-zone field at `center` (the four Cryo/Stasis/Storm/Pyre
/// abilities). `timer: 0.0` so it applies its status on the very next tick.
pub fn spawn_ability_field(
    commands: &mut Commands,
    center: Vec2,
    status: FieldStatus,
    radius: f32,
    secs: f32,
    tick: f32,
) {
    commands.spawn((
        AbilityField {
            status,
            radius,
            secs,
            tick,
            timer: 0.0,
        },
        field_shape(radius, status),
        Transform::from_translation(center.extend(0.4)),
    ));
}

/// A translucent filled disc tinted to the field's element (HDR for a soft bloom).
fn field_shape(radius: f32, status: FieldStatus) -> Shape {
    let (r, g, b) = match status {
        FieldStatus::Freeze => (0.5, 0.85, 1.0),
        FieldStatus::Chill => (0.65, 0.72, 1.0),
        FieldStatus::Conduct => (1.0, 0.9, 0.3),
        FieldStatus::Burn => (1.0, 0.45, 0.15),
    };
    let mut path = ShapePath::new();
    let segments = 40;
    for i in 0..segments {
        let a = i as f32 / segments as f32 * TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::srgba(r, g, b, 0.12))
        .build()
}

/// Drop-zone fields: each tick interval, re-apply the field's status to every
/// enemy inside its radius; despawn the field when its lifetime elapses.
pub fn tick_ability_fields(
    time: Res<Time>,
    mut commands: Commands,
    mut fields: Query<(Entity, &Transform, &mut AbilityField)>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    let dt = time.delta_secs();
    for (field_e, ftf, mut field) in &mut fields {
        field.secs -= dt;
        if field.secs <= 0.0 {
            commands.entity(field_e).despawn();
            continue;
        }
        field.timer -= dt;
        if field.timer > 0.0 {
            continue;
        }
        field.timer = field.tick;

        let center = ftf.translation.truncate();
        let r2 = field.radius * field.radius;
        for (e, etf) in &enemies {
            if etf.translation.truncate().distance_squared(center) > r2 {
                continue;
            }
            match field.status {
                FieldStatus::Freeze => {
                    commands.entity(e).insert(Frozen { secs: FREEZE_SECS });
                }
                FieldStatus::Chill => {
                    commands.entity(e).insert(Chill { secs: CHILL_SECS });
                }
                FieldStatus::Conduct => {
                    commands.entity(e).insert(Conduct { secs: CONDUCT_SECS });
                }
                FieldStatus::Burn => {
                    commands.entity(e).insert(Burning { dps: 6.0, secs: 1.0 });
                }
            }
        }
    }
}
