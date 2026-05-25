//! EN enemy special mechanics (Phase EN) — the signature behaviors beyond the
//! shared movement/fire patterns. Each hooks a death (death-flare, split) or
//! runs on a per-enemy timer (spawner, acid trail). Built incrementally; the
//! death-hooked ones land first (they reuse the existing `Death` message).

use crate::combat::element::Element;
use crate::components::{EnemyKind, PlayerCorrode, Ship};
use crate::messages::{Damage, Death};
use crate::systems::player_status::apply_player_status;
use bevy::prelude::*;

/// Ashen Detonator death-flare radius (enemy-data.js: 130 px).
pub const ASHEN_FLARE_RADIUS: f32 = 130.0;
/// Ashen Detonator death-flare damage (enemy-data.js: 12).
pub const ASHEN_FLARE_DAMAGE: f32 = 12.0;

/// On an **Ashen Detonator**'s death, a PYRO blast: if the player is within the
/// flare radius, it takes [`ASHEN_FLARE_DAMAGE`] + a burn (the `deathFlare`
/// mechanic). Runs after `apply_damage` (so it sees the `Death`); the `Damage`
/// it writes lands the next tick, like the other death-driven effects.
pub fn ashen_death_flare(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut dmg: MessageWriter<Damage>,
    player: Query<(Entity, &Transform, Option<&PlayerCorrode>), With<Ship>>,
) {
    let Ok((player_e, ptf, pcorrode)) = player.single() else {
        return;
    };
    let ppos = ptf.translation.truncate();
    let corrode = pcorrode.map_or(0, |c| c.stacks);
    for d in deaths.read() {
        if d.kind != Some(EnemyKind::AshenDetonator) {
            continue;
        }
        if d.position.distance(ppos) <= ASHEN_FLARE_RADIUS {
            dmg.write(Damage {
                target: player_e,
                amount: ASHEN_FLARE_DAMAGE,
            });
            apply_player_status(&mut commands, player_e, Element::Pyro, corrode);
        }
    }
}
