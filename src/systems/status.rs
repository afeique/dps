//! Status effects on enemies. The two original verbs — `Burning` DoT and
//! `Stunned` fire-suppression — plus the Phase-E3 elemental statuses (`Bleed`
//! DoT, and the simple-timer `Chill`/`Frozen`/`Conduct`/`Corrode`/`Mark`/`Oil`).
//! All tick down in `FixedUpdate` and remove themselves on expiry. DoT verbs
//! (`Burning`, `Bleed`) emit `Damage` so they run in the collision group before
//! `apply_damage`; fire-suppression (`Stunned`, `Frozen`) is read by
//! `enemy::firing`; the rest are read by the damage/movement paths as elemental
//! sources land (W/E5).

use crate::components::{Bleed, Burning, Chill, Conduct, Corrode, Frozen, Mark, Oil, Stunned};
use crate::messages::Damage;
use bevy::prelude::*;

/// Apply each `Burning`'s `dps × dt` as `Damage` and expire it.
pub fn tick_burning(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut q: Query<(Entity, &mut Burning)>,
) {
    let dt = time.delta_secs();
    for (e, mut burn) in &mut q {
        dmg.write(Damage {
            target: e,
            amount: burn.dps * dt,
        });
        burn.secs -= dt;
        if burn.secs <= 0.0 {
            commands.entity(e).remove::<Burning>();
        }
    }
}

/// Count down each `Stunned` and remove it on expiry.
pub fn tick_stun(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Stunned)>) {
    let dt = time.delta_secs();
    for (e, mut stun) in &mut q {
        stun.secs -= dt;
        if stun.secs <= 0.0 {
            commands.entity(e).remove::<Stunned>();
        }
    }
}

/// Apply each `Bleed`'s `dps × dt` as `Damage` and expire it (TOXIC poison —
/// the no-refresh sibling of `Burning`). Runs in the collision group, before
/// `apply_damage`.
pub fn tick_bleed(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut q: Query<(Entity, &mut Bleed)>,
) {
    let dt = time.delta_secs();
    for (e, mut bleed) in &mut q {
        dmg.write(Damage {
            target: e,
            amount: bleed.dps * dt,
        });
        bleed.secs -= dt;
        if bleed.secs <= 0.0 {
            commands.entity(e).remove::<Bleed>();
        }
    }
}

/// Count down the simple-timer elemental statuses (`Chill` / `Frozen` /
/// `Conduct` / `Corrode` / `Mark` / `Oil`) and remove each on expiry. Their
/// *effects* (chill slow, freeze halt + fire-suppress, conduct/corrode amplify,
/// void mark) are read by the movement/damage paths; this only manages their
/// lifetimes. Six disjoint component types → no query conflict.
pub fn tick_status_timers(
    time: Res<Time>,
    mut commands: Commands,
    mut chill: Query<(Entity, &mut Chill)>,
    mut frozen: Query<(Entity, &mut Frozen)>,
    mut conduct: Query<(Entity, &mut Conduct)>,
    mut corrode: Query<(Entity, &mut Corrode)>,
    mut mark: Query<(Entity, &mut Mark)>,
    mut oil: Query<(Entity, &mut Oil)>,
) {
    let dt = time.delta_secs();
    for (e, mut s) in &mut chill {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<Chill>();
        }
    }
    for (e, mut s) in &mut frozen {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<Frozen>();
        }
    }
    for (e, mut s) in &mut conduct {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<Conduct>();
        }
    }
    for (e, mut s) in &mut corrode {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<Corrode>();
        }
    }
    for (e, mut s) in &mut mark {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<Mark>();
        }
    }
    for (e, mut s) in &mut oil {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<Oil>();
        }
    }
}
