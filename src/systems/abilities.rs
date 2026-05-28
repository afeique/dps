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
            // Deferred abilities (effects land in later increments).
            _ => false,
        };

        if fired {
            cds.trigger(slot, ability.cooldown());
        }
    }
}
