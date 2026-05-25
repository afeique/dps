//! Player-side elemental statuses (Phase E5) — the ship analog of
//! `systems::status` (which handles enemy statuses). Ported from
//! `player-status.js`: an enemy attack's element stamps a debuff on the ship —
//! PYRO→burn DoT, CRYO→chill (slow), TOXIC→corrode (amplifies incoming damage).
//! VOLT/VOID/RADIANT apply none.
//!
//! `apply_player_status` is called from the hit sites (E5: enemy contact;
//! E5b adds enemy bullets once they carry an element). `tick_player_burn` deals
//! the burn DoT (collision group, before `apply_damage`); `tick_player_statuses`
//! counts the chill/corrode timers down. The chill movement-slow + corrode
//! damage-amplify *effects* are wired in E5b — this lands the subsystem + the
//! live burn DoT.

use crate::combat::element::Element;
use crate::components::{
    PlayerBurn, PlayerChill, PlayerCorrode, PLAYER_BURN_PER_TICK, PLAYER_BURN_SECS,
    PLAYER_BURN_TICK_SECS, PLAYER_CHILL_SECS, PLAYER_CORRODE_MAX, PLAYER_CORRODE_SECS,
};
use crate::messages::Damage;
use bevy::prelude::*;

/// Stamp `element`'s signature status onto the player. `prev_corrode` is the
/// player's current corrode stacks (so TOXIC stacks up to the cap); pass 0 if none.
pub fn apply_player_status(
    commands: &mut Commands,
    player: Entity,
    element: Element,
    prev_corrode: u32,
) {
    match element {
        Element::Pyro => {
            commands.entity(player).insert(PlayerBurn {
                secs: PLAYER_BURN_SECS,
                tick: PLAYER_BURN_TICK_SECS,
            });
        }
        Element::Cryo => {
            commands.entity(player).insert(PlayerChill {
                secs: PLAYER_CHILL_SECS,
            });
        }
        Element::Toxic => {
            commands.entity(player).insert(PlayerCorrode {
                stacks: (prev_corrode + 1).min(PLAYER_CORRODE_MAX),
                secs: PLAYER_CORRODE_SECS,
            });
        }
        Element::Kinetic | Element::Volt | Element::Void | Element::Radiant => {}
    }
}

/// Apply the player's `PlayerBurn` DoT in 500 ms chunks (so each `Damage`
/// survives the player-damage rounding) and expire it. Runs in the collision
/// group so its `Damage` reaches `apply_damage` this tick.
pub fn tick_player_burn(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut q: Query<(Entity, &mut PlayerBurn)>,
) {
    let dt = time.delta_secs();
    for (e, mut burn) in &mut q {
        burn.tick -= dt;
        burn.secs -= dt;
        if burn.tick <= 0.0 {
            dmg.write(Damage {
                target: e,
                amount: PLAYER_BURN_PER_TICK,
            });
            burn.tick += PLAYER_BURN_TICK_SECS;
        }
        if burn.secs <= 0.0 {
            commands.entity(e).remove::<PlayerBurn>();
        }
    }
}

/// Count down the player's `PlayerChill` / `PlayerCorrode` timers and remove each
/// on expiry. (Their movement-slow / damage-amplify effects land in E5b.)
pub fn tick_player_statuses(
    time: Res<Time>,
    mut commands: Commands,
    mut chill: Query<(Entity, &mut PlayerChill)>,
    mut corrode: Query<(Entity, &mut PlayerCorrode)>,
) {
    let dt = time.delta_secs();
    for (e, mut c) in &mut chill {
        c.secs -= dt;
        if c.secs <= 0.0 {
            commands.entity(e).remove::<PlayerChill>();
        }
    }
    for (e, mut c) in &mut corrode {
        c.secs -= dt;
        if c.secs <= 0.0 {
            commands.entity(e).remove::<PlayerCorrode>();
        }
    }
}
