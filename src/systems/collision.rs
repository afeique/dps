//! Collision. Phase 1 implements three pairs:
//!   • player bullets → enemies (closes the original slice loop)
//!   • enemy bullets  → player
//!   • enemy contact  → player (hull overlap, enemy survives)
//! All pairs are naive O(n × m). A `Damage` message is emitted on contact;
//! bullets are despawned; enemy drifters are NOT despawned on contact.
//!
//! Phase 3 replaces this with the ported spatial-grid broadphase
//! (`js/modules/performance/spatial-grid.js` + `combat/collision-system.js`)
//! and adds the remaining pairs (AOE rings, asteroids).

use crate::combat::element::{Element, Resistances};
use crate::combat::reaction::{flare_damage, shatter_triggers, PendingReactions, ReactionSeed};
use crate::components::*;
use crate::messages::{Damage, Knockback};
use crate::resources::{crit_chance, roll_crit, EnergyMeter, GameRng, ENERGY_PER_HIT};
use crate::systems::asteroids::Asteroid;
use crate::systems::items::{AffixKind, Equipment};
use crate::systems::shop::{
    amplifier_mult, executioner_bonus, explosion_radius, glass_cannon_dmg, knock_chance,
    berserker_bonus, frenzy_bonus, gunslinger_dmg, opportunist_bonus, overflow_bonus,
    predator_bonus, stun_chance, vampiric_rounds_heal, vampirism_frac, UpgradeId, Upgrades,
    EXECUTE_THRESHOLD, KNOCK_PX, PREDATOR_THRESHOLD,
};
use bevy::prelude::*;

/// Enemy-side defense layer applied to a post-attack-multiplier hit `amount`
/// (E4 — the `applyDamageToEnemy` order, collision-system.js:2437-2478):
/// CORRODE amplify (+15%/stack) → CONDUCT (VOLT ×1.5 vs a conducting target) →
/// **PURGE gate** (a RADIANT hit skips armor + frontal shield) → flat ARMOR
/// (25% floor) → frontal-shield reduction. Pure + unit-tested.
pub fn enemy_defense_damage(
    amount: f32,
    corrode_stacks: u32,
    conducting: bool,
    has_volt: bool,
    has_radiant: bool,
    armor: f32,
    frontal_reduction: f32,
    frontal_blocked: bool,
) -> f32 {
    let mut d = amount * (1.0 + CORRODE_PER_STACK * corrode_stacks as f32);
    if conducting && has_volt {
        d *= CONDUCT_VOLT_MULT;
    }
    if !has_radiant {
        if armor > 0.0 {
            d = (d * ARMOR_FLOOR).max(d - armor);
        }
        if frontal_blocked {
            d *= 1.0 - frontal_reduction;
        }
    }
    d
}

/// Frontal-shield block test (E4): true when the hit arrives within `arc/2` of
/// the enemy→player bearing — so direct shots from the player are blocked while
/// flanking / wall-bounced / returning shots get through. `Vec2::ZERO` bearings
/// (coincident points) never block.
pub fn frontal_blocked(enemy_pos: Vec2, player_pos: Vec2, hit_pos: Vec2, arc: f32) -> bool {
    let to_player = (player_pos - enemy_pos).normalize_or_zero();
    let to_hit = (hit_pos - enemy_pos).normalize_or_zero();
    if to_player == Vec2::ZERO || to_hit == Vec2::ZERO {
        return false;
    }
    to_player.angle_to(to_hit).abs() < arc * 0.5
}

/// Elemental status-on-hit tuning (E5b). A non-Kinetic player/tower bullet stamps
/// its element's signature status on the enemy, which is what lets towers combo:
/// a Frost (Cryo) shot freezes its target, so the next heavy hit (Inferno/Gun)
/// SHATTERS it; Toxic stacks corrode vulnerability; Volt makes it conduct; etc.
/// Burn DoT is a fraction of the landed hit, floored so chip shots still tick.
const STATUS_BURN_DPS_FRAC: f32 = 0.5;
const STATUS_BURN_MIN_DPS: f32 = 3.0;
const STATUS_BURN_SECS: f32 = 2.0;

pub fn bullet_hits_enemy(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut knock: MessageWriter<Knockback>,
    mut crits: MessageWriter<crate::messages::Crit>,
    mut shards: MessageWriter<crate::messages::Shard>,
    upgrades: Res<Upgrades>,
    equipment: Res<Equipment>,
    meta: Res<crate::meta::Meta>,
    mut rng: ResMut<GameRng>,
    mut energy: ResMut<EnergyMeter>,
    mut bullets: Query<(
        Entity,
        &Transform,
        &Collider,
        &mut Bullet,
        Option<&BulletElements>,
        Option<&mut Velocity>,
        Option<&mut Bounce>,
        Option<&MitosisGen>,
    )>,
    // `Without<Ship>` keeps this immut `&Health` disjoint from `player_hp`'s mut.
    // The element components (E2/E4) are optional so test-spawned enemies (no
    // resist/armor) are unaffected (neutral ×1).
    enemies: Query<
        (
            Entity,
            &Transform,
            &Collider,
            &Health,
            Option<&Resistances>,
            Option<&Corrode>,
            Option<&Conduct>,
            Option<&Armor>,
            Option<&FrontalShield>,
            Has<Frozen>,
            Has<Oil>,
            Option<&AllyShield>,
            Has<Adaptive>,
            Has<CoreShielded>,
        ),
        (With<Enemy>, Without<Ship>),
    >,
    mut player_hp: Query<&mut Health, With<Ship>>,
    // Player position for the frontal-shield bearing test (E4). Disjoint from
    // `player_hp` (different component) so no query conflict.
    player_pos: Query<&Transform, With<Ship>>,
    // Reaction seeds (E4b) — shatter/oil-flare detected here, resolved by
    // `reactions::resolve_reactions` after this system.
    mut reactions: ResMut<PendingReactions>,
) {
    let player_pos_v = player_pos.single().ok().map(|t| t.translation.truncate());
    // BERSERKER passive reads the player's HP fraction once (≈constant this tick);
    // its damage bonus grows as HP drops. Immutable borrow ends before the loop's
    // mutable `player_hp` use below.
    let player_hp_frac = player_hp
        .iter()
        .next()
        .map(|h| if h.max > 0.0 { h.current / h.max } else { 1.0 })
        .unwrap_or(1.0);
    // VAMPIRISM passive: heal a fraction of damage dealt (spec III.5) — shop
    // stacks + equipped item affixes.
    // Each adds the account SP-stat bonus (sp_value is a % → /100) on top of the
    // shop stacks + equipped item affixes.
    let vamp = vampirism_frac(upgrades.owned(UpgradeId::Vampirism))
        + equipment.affix_total(AffixKind::Vampirism) / 100.0
        + meta.sp_value("VAMPIRISM") / 100.0;
    // VAMPIRIC ROUNDS passive: a flat HP refund on each critical hit.
    let crit_heal = vampiric_rounds_heal(upgrades.owned(UpgradeId::VampiricRounds));
    // Crit chance/damage scale with their upgrade stacks (spec III.6) + equipped
    // item affixes (chance as a fraction; damage as an additive bonus on the cap).
    let crit_p = crit_chance(upgrades.owned(UpgradeId::CritChance))
        + equipment.affix_total(AffixKind::CritChance) / 100.0
        + meta.sp_value("CRIT_CHANCE") / 100.0;
    let crit_dmg_stacks = upgrades.owned(UpgradeId::CritDamage);
    let crit_dmg_bonus =
        equipment.affix_total(AffixKind::CritDamage) / 100.0 + meta.sp_value("CRIT_DAMAGE") / 100.0;
    // FRENZY scaled with the (now-removed) kill-streak; with no streak it has no
    // kills to ride on, so its bonus stays inert (0). Kept so the upgrade survives
    // a future kill-streak reimplementation.
    let frenzy_kills = 0;
    // OVERFLOW: a full energy meter buffs primary damage (rewards holding charge).
    let energy_full = energy.current >= energy.max * 0.999;
    let overflow = if energy_full {
        overflow_bonus(upgrades.owned(UpgradeId::Overflow))
    } else {
        0.0
    };
    // AMPLIFIER (+%/stack) + GLASS CANNON (+50% flat) + BERSERKER (+%/stack scaled
    // by missing HP) + FRENZY (+%/stack scaled by streak) + OVERFLOW (+%/stack at
    // full energy) damage multipliers.
    let amp = amplifier_mult(upgrades.owned(UpgradeId::Amplifier))
        + glass_cannon_dmg(upgrades.owned(UpgradeId::GlassCannon))
        + berserker_bonus(upgrades.owned(UpgradeId::Berserker), player_hp_frac)
        + frenzy_bonus(upgrades.owned(UpgradeId::Frenzy), frenzy_kills)
        + overflow
        + gunslinger_dmg(upgrades.owned(UpgradeId::Gunslinger));
    // `_STUN` bullet trait: chance to stun the enemy on hit (spec III.2/III.6).
    let stun_p = stun_chance(upgrades.owned(UpgradeId::StunShot));
    // `_EXPLODE` bullet trait: AoE splash radius on hit (0 = off).
    let explode_r = explosion_radius(upgrades.owned(UpgradeId::ExplodeShot));
    // `_KNOCK` bullet trait: chance to shove the enemy on hit.
    let knock_p = knock_chance(upgrades.owned(UpgradeId::KnockShot));
    // EXECUTIONER passive: bonus damage vs enemies below the execute threshold.
    let exec_bonus = executioner_bonus(upgrades.owned(UpgradeId::Executioner));
    // PREDATOR passive: bonus damage vs healthy enemies (above PREDATOR_THRESHOLD).
    let pred_bonus = predator_bonus(upgrades.owned(UpgradeId::Predator));
    // OPPORTUNIST passive: bonus damage vs status-afflicted enemies.
    let opp_bonus = opportunist_bonus(upgrades.owned(UpgradeId::Opportunist));
    for (bullet_e, btf, bc, mut bullet, belems, bvel, bounce, mgen) in &mut bullets {
        if bullet.kind != BulletKind::Player {
            continue;
        }
        // The bullet's resolved element set (E2); absent ⇒ neutral (no resist).
        let belem_set = belems.map(|b| b.0);
        let has_volt = belem_set.is_some_and(|s| s.contains(Element::Volt));
        let has_radiant = belem_set.is_some_and(|s| s.contains(Element::Radiant));
        let has_pyro = belem_set.is_some_and(|s| s.contains(Element::Pyro));
        for (
            enemy_e,
            etf,
            ec,
            ehp,
            eres,
            ecorrode,
            econduct,
            earmor,
            efrontal,
            efrozen,
            eoil,
            eshield,
            eadaptive,
            eshielded,
        ) in &enemies
        {
            // A boss core is invulnerable while its weak-point parts live (BO):
            // the shot passes through it (and may still hit a part this frame).
            if eshielded {
                continue;
            }
            let reach = bc.radius + ec.radius;
            let d2 = btf
                .translation
                .truncate()
                .distance_squared(etf.translation.truncate());
            if d2 <= reach * reach {
                // Streak multiplier × per-hit crit roll (spec III.6).
                let crit_mult = roll_crit(&mut rng, crit_p, crit_dmg_stacks, crit_dmg_bonus);
                if crit_mult > 1.0 {
                    // feeds the precision mission + the crit SFX + the "CRIT!" text
                    crits.write(crate::messages::Crit { pos: etf.translation.truncate() });
                }
                // EXECUTIONER: extra damage vs an enemy already below 25% HP.
                let exec = if exec_bonus > 0.0 && ehp.current < ehp.max * EXECUTE_THRESHOLD {
                    1.0 + exec_bonus
                } else {
                    1.0
                };
                // PREDATOR: extra damage vs a still-healthy enemy (>75% HP).
                let predator = if pred_bonus > 0.0 && ehp.current > ehp.max * PREDATOR_THRESHOLD {
                    1.0 + pred_bonus
                } else {
                    1.0
                };
                // OPPORTUNIST: extra damage vs a status-afflicted enemy.
                let afflicted = efrozen || eoil || ecorrode.is_some() || econduct.is_some();
                let opportunist = if opp_bonus > 0.0 && afflicted {
                    1.0 + opp_bonus
                } else {
                    1.0
                };
                // Element/resistance multiplier (E2): the AVERAGE of the bullet's
                // elements vs this enemy's resist map (resist <1, weakness >1).
                let resist_mult = match (belem_set, eres) {
                    (Some(set), Some(res)) => res.multi_multiplier_set(set),
                    _ => 1.0,
                };
                // Enemy-side defense layer (E4): corrode/conduct amplify, then the
                // RADIANT-purge-gated flat armor + frontal shield.
                let blocked = match (efrontal, player_pos_v) {
                    (Some(fs), Some(ppos)) => frontal_blocked(
                        etf.translation.truncate(),
                        ppos,
                        btf.translation.truncate(),
                        fs.arc,
                    ),
                    _ => false,
                };
                // Lumen Drone ally-shield: incoming damage ×(1 − amount) (EN).
                let ally = eshield.map_or(1.0, |s| 1.0 - s.amount);
                let amount = enemy_defense_damage(
                    bullet.damage
                        * amp
                        * crit_mult
                        * exec
                        * predator
                        * opportunist
                        * resist_mult
                        * ally,
                    ecorrode.map_or(0, |c| c.stacks),
                    econduct.is_some(),
                    has_volt,
                    has_radiant,
                    earmor.map_or(0.0, |a| a.0),
                    efrontal.map_or(0.0, |f| f.reduction),
                    blocked,
                );
                dmg.write(Damage {
                    target: enemy_e,
                    amount,
                });
                // Elemental reactions (E4b): a heavy hit on a frozen enemy
                // shatters into its neighbors; a PYRO hit on an oiled enemy
                // flares. Resolved by `reactions::resolve_reactions` this tick.
                let center = etf.translation.truncate();
                if shatter_triggers(efrozen, amount, 0) {
                    reactions.0.push(ReactionSeed::Shatter {
                        source: enemy_e,
                        center,
                        depth: 0,
                    });
                }
                if eoil && has_pyro {
                    reactions.0.push(ReactionSeed::Flare {
                        center,
                        damage: flare_damage(amount),
                    });
                }
                // Warden adaptive resist (EN): after a hit, bump a copy of the
                // enemy's resist toward each element carried, so the NEXT
                // same-element hit does less. Deferred via Commands → no
                // mut-`Resistances` borrow in this (shared, splash-iterated) query.
                if eadaptive {
                    if let (Some(set), Some(res)) = (belem_set, eres) {
                        let mut bumped = *res;
                        for el in set.iter() {
                            bumped.adapt_default(el);
                        }
                        commands.entity(enemy_e).insert(bumped);
                    }
                }
                // VAMPIRISM heals a fraction of every hit's damage; VAMPIRIC
                // ROUNDS adds a flat refund on crits. (Over-fill allowed;
                // overheal_to_tanks converts the overflow.)
                let heal = amount * vamp + if crit_mult > 1.0 { crit_heal } else { 0.0 };
                if heal > 0.0 {
                    if let Ok(mut hp) = player_hp.single_mut() {
                        hp.current += heal;
                    }
                }
                // Landing a hit charges the power-weapon energy meter (spec III.3);
                // SP REACTOR boosts the gain (% energy regen).
                energy.gain(ENERGY_PER_HIT * (1.0 + meta.sp_value("REACTOR") / 100.0));
                // `_STUN` trait proc: briefly stun the enemy (spec III.6).
                if stun_p > 0.0 && rng.next_f32() < stun_p {
                    commands.entity(enemy_e).insert(Stunned { secs: 1.0 });
                }
                // Elemental status-on-hit (E5b): the bullet's element stamps its
                // signature status, enabling tower combos. Inserted via Commands,
                // so it lands NEXT tick — the current hit uses the prior state
                // (so a Frost shot freezes, and a later heavy hit shatters).
                if let Some(set) = belem_set {
                    for el in set.iter() {
                        match el {
                            Element::Pyro => {
                                commands.entity(enemy_e).insert(Burning {
                                    dps: (amount * STATUS_BURN_DPS_FRAC).max(STATUS_BURN_MIN_DPS),
                                    secs: STATUS_BURN_SECS,
                                });
                            }
                            Element::Cryo => {
                                commands.entity(enemy_e).insert(Frozen { secs: FREEZE_SECS });
                            }
                            Element::Volt => {
                                commands.entity(enemy_e).insert(Conduct { secs: CONDUCT_SECS });
                            }
                            Element::Toxic => {
                                let stacks = (ecorrode.map_or(0, |c| c.stacks) + 1)
                                    .min(CORRODE_MAX_STACKS);
                                commands
                                    .entity(enemy_e)
                                    .insert(Corrode { stacks, secs: CORRODE_SECS });
                            }
                            Element::Void => {
                                commands.entity(enemy_e).insert(Mark { secs: MARK_SECS });
                            }
                            // Kinetic has no status; Radiant's purge is a per-hit
                            // damage gate (handled in `enemy_defense_damage`).
                            Element::Kinetic | Element::Radiant => {}
                        }
                    }
                }
                // `_KNOCK` trait proc: shove the enemy away from the impact.
                if knock_p > 0.0 && rng.next_f32() < knock_p {
                    let dir = (etf.translation.truncate() - btf.translation.truncate())
                        .normalize_or_zero();
                    if dir != Vec2::ZERO {
                        knock.write(Knockback {
                            target: enemy_e,
                            impulse: dir * KNOCK_PX,
                        });
                    }
                }
                // `_EXPLODE` trait: splash the (no-crit) bullet damage to every
                // other enemy within the blast radius.
                if explode_r > 0.0 {
                    let hit_pos = etf.translation.truncate();
                    let splash = bullet.damage * amp;
                    for (e2, etf2, ec2, _ehp2, eres2, _, _, _, _, _, _, _, _, _) in &enemies {
                        if e2 == enemy_e {
                            continue;
                        }
                        if etf2.translation.truncate().distance(hit_pos) <= explode_r + ec2.radius {
                            // The splash takes each splashed enemy's own resist (E2).
                            let smult = match (belem_set, eres2) {
                                (Some(set), Some(res)) => res.multi_multiplier_set(set),
                                _ => 1.0,
                            };
                            dmg.write(Damage {
                                target: e2,
                                amount: splash * smult,
                            });
                        }
                    }
                }
                // Mitosis (W): fragment into shards on impact, carrying gen−1
                // (read the velocity here, before the bounce match consumes it).
                if let Some(mg) = mgen {
                    if mg.0 > 0 {
                        let vdir = bvel.as_ref().map_or(Vec2::Y, |v| v.0.normalize_or_zero());
                        let vspeed = bvel.as_ref().map_or(300.0, |v| v.0.length());
                        let elem = belem_set.unwrap_or(crate::combat::element::ElementSet::kinetic());
                        let n = crate::systems::weapons::MITOSIS_SPLIT_COUNT;
                        for i in 0..n {
                            // Even fan across ±MITOSIS_SPREAD around the travel dir.
                            let f = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
                            let off = -crate::systems::weapons::MITOSIS_SPREAD
                                + f * (2.0 * crate::systems::weapons::MITOSIS_SPREAD);
                            let (s, c) = off.sin_cos();
                            let dir = Vec2::new(c * vdir.x - s * vdir.y, s * vdir.x + c * vdir.y);
                            shards.write(crate::messages::Shard {
                                origin: btf.translation.truncate(),
                                dir,
                                speed: vspeed * crate::systems::weapons::MITOSIS_SPEED_FACTOR,
                                damage: bullet.damage * crate::systems::weapons::MITOSIS_DMG_FACTOR,
                                element: elem,
                                generation: mg.0 - 1,
                            });
                        }
                    }
                }
                // Caroms (W): redirect toward the nearest OTHER enemy within the
                // seek radius instead of dying, up to `remaining` bounces.
                let bounced = match (bounce, bvel) {
                    (Some(mut b), Some(mut vel)) if b.remaining > 0 => {
                        let from = btf.translation.truncate();
                        let r2 = b.seek_radius * b.seek_radius;
                        let mut best: Option<(f32, Vec2)> = None;
                        for (e2, etf2, ..) in &enemies {
                            if e2 == enemy_e {
                                continue;
                            }
                            let p = etf2.translation.truncate();
                            let d2 = p.distance_squared(from);
                            if d2 <= r2 && best.is_none_or(|(bd, _)| d2 < bd) {
                                best = Some((d2, p));
                            }
                        }
                        match best {
                            Some((_, target)) => {
                                let speed = vel.0.length();
                                let dir = (target - from).normalize_or_zero();
                                if dir != Vec2::ZERO && speed > 0.0 {
                                    vel.0 = dir * speed;
                                }
                                b.remaining -= 1;
                                true // bounced — survives this hit
                            }
                            None => false, // no target in range → expire normally
                        }
                    }
                    _ => false,
                };
                // Piercing bullets pass through; others die on the first hit.
                // (One hit per frame — a fast bullet clears an enemy's radius
                // before the next tick, so re-hits are rare. Tracking a hit-set
                // for slow piercers is a later refinement.)
                if !bounced {
                    if bullet.pierce == 0 {
                        commands.entity(bullet_e).despawn();
                    } else {
                        bullet.pierce -= 1;
                    }
                }
                break;
            }
        }
    }
}

pub fn enemy_bullet_hits_player(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    player: Query<
        (Entity, &Transform, &Collider, Option<&Resistances>, Option<&PlayerCorrode>),
        With<Ship>,
    >,
    bullets: Query<(Entity, &Transform, &Collider, &Bullet, Option<&BulletElements>)>,
) {
    let Ok((player_e, ptf, pc, presist, pcorrode)) = player.single() else {
        return;
    };
    let corrode_stacks = pcorrode.map_or(0, |c| c.stacks);
    for (bullet_e, btf, bc, bullet, belems) in &bullets {
        if bullet.kind != BulletKind::Enemy {
            continue;
        }
        let reach = bc.radius + pc.radius;
        let d2 = btf
            .translation
            .truncate()
            .distance_squared(ptf.translation.truncate());
        if d2 <= reach * reach {
            // Elemental enemy bullet (E8): scale by the player's resist for the
            // bullet's element + stamp its signature status (single-element).
            let elem = belems.and_then(|b| b.0.iter().next());
            let resist_mult = match (elem, presist) {
                (Some(e), Some(r)) => r.player_multiplier(e),
                _ => 1.0,
            };
            dmg.write(Damage {
                target: player_e,
                amount: bullet.damage * resist_mult,
            });
            if let Some(e) = elem {
                crate::systems::player_status::apply_player_status(
                    &mut commands,
                    player_e,
                    e,
                    corrode_stacks,
                );
            }
            commands.entity(bullet_e).despawn();
        }
    }
}

/// Apply queued `Knockback` shoves (the `_KNOCK` trait): nudge each target's
/// position by its impulse. A separate system so producers (e.g.
/// `bullet_hits_enemy`) needn't hold a mutable `Transform` handle.
pub fn apply_knockback(mut knock: MessageReader<Knockback>, mut q: Query<&mut Transform>) {
    for k in knock.read() {
        if let Ok(mut tf) = q.get_mut(k.target) {
            tf.translation.x += k.impulse.x;
            tf.translation.y += k.impulse.y;
        }
    }
}

/// Enemy→player contact damage (`getLevelScaledDamage(25) = 25` at level 1,
/// spec III.6 — in-run leveling is retired so it never scales).
const CONTACT_DAMAGE: f32 = 25.0;
/// Player→enemy contact damage (`PLAYER_ENEMY_COLLISION_DAMAGE`, spec III.6).
const PLAYER_ENEMY_CONTACT_DMG: f32 = 5.0;
/// Each body is pushed out by this fraction of the overlap on contact
/// (`OVERLAP_SEPARATION_RATIO`, spec III.6). Applied to *both* bodies, total
/// push = `1.2 × overlap` → they always separate the same tick. This (not
/// post-hit i-frames, which were removed) is what gates contact damage to one
/// hit per collision.
const OVERLAP_SEPARATION_RATIO: f32 = 0.6;
/// Bounciness of a contact's momentum impulse (`BOUNCE_RESTITUTION`): 0.8 ≈ a
/// springy near-elastic exchange of velocity along the collision normal.
const CONTACT_RESTITUTION: f32 = 0.8;
/// A small fixed separation pop added on top of the impulse so even a glancing or
/// stationary contact still parts with a bit of juice.
const CONTACT_POP: f32 = 70.0;

/// Player ↔ enemy contact (spec III.6): the player takes 25, the enemy takes 5,
/// and both are physically separated + bounced apart. There is **no** post-hit
/// invulnerability (spec II.2) — the separation is what stops a single overlap
/// from melting the player over consecutive ticks.
pub fn enemy_contact_player(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut player: Query<
        (
            Entity,
            &mut Transform,
            &mut Velocity,
            &Collider,
            Option<&PlayerCorrode>,
            Option<&Resistances>,
        ),
        With<Ship>,
    >,
    mut enemies: Query<(Entity, &Enemy, &mut Transform, &mut Velocity, &Collider), (With<Enemy>, Without<Ship>)>,
) {
    let Ok((player_e, mut ptf, mut pvel, pc, pcorrode, presist)) = player.single_mut() else {
        return;
    };
    let corrode_stacks = pcorrode.map_or(0, |c| c.stacks);
    for (enemy_e, enemy, mut etf, mut evel, ec) in &mut enemies {
        // Re-read the player position each enemy: a prior separation may have
        // already nudged it this tick.
        let ppos = ptf.translation.truncate();
        let epos = etf.translation.truncate();
        let reach = pc.radius + ec.radius;
        let delta = ppos - epos;
        let dist = delta.length();
        if dist >= reach {
            continue; // not overlapping
        }

        // Damage both ways (the pipeline applies the player's shield + rounding;
        // a weak enemy may die to the 5-dmg ram). The contact is typed by the
        // enemy's element (E5/E8) and scaled by the player's elemental resist
        // (item resist affixes feed `Resistances`; neutral until they land).
        let contact_elem = crate::systems::enemy::element_for(enemy.kind);
        let resist_mult = presist.map_or(1.0, |r| r.player_multiplier(contact_elem));
        dmg.write(Damage { target: player_e, amount: CONTACT_DAMAGE * resist_mult });
        dmg.write(Damage { target: enemy_e, amount: PLAYER_ENEMY_CONTACT_DMG });

        // Elemental contact (E5): the enemy's attack element stamps its signature
        // status on the ship (e.g. a Tangerine's PYRO ram → burn). Bullet-borne
        // elemental status follows in E5b.
        crate::systems::player_status::apply_player_status(
            &mut commands,
            player_e,
            crate::systems::enemy::element_for(enemy.kind),
            corrode_stacks,
        );

        // Normal from enemy → player (fall back to +Y when exactly coincident).
        let n = if dist > 1e-4 { delta / dist } else { Vec2::Y };

        // Positional separation — push both out of the overlap this tick.
        let push = n * ((reach - dist) * OVERLAP_SEPARATION_RATIO);
        ptf.translation.x += push.x;
        ptf.translation.y += push.y;
        etf.translation.x -= push.x;
        etf.translation.y -= push.y;

        // Momentum-based collision response: an elastic impulse along the normal,
        // with mass ∝ area (radius²) so a big enemy shoves the ship hard while a
        // small drone barely budges it. Only fires when the bodies are actually
        // approaching (closing speed along `n`), so it exchanges momentum rather
        // than injecting energy; a small fixed pop guarantees clean separation.
        let (mp, me) = (pc.radius * pc.radius, ec.radius * ec.radius);
        let closing = (pvel.0 - evel.0).dot(n);
        if closing < 0.0 {
            let j = -(1.0 + CONTACT_RESTITUTION) * closing / (1.0 / mp + 1.0 / me);
            pvel.0 += (j / mp) * n;
            evel.0 -= (j / me) * n;
        }
        pvel.0 += n * CONTACT_POP;
        evel.0 -= n * CONTACT_POP;
    }
}

/// Per-enemy "leak" damage to the Core when one reaches it. Bosses punch far
/// harder, so letting one breach is a serious blow against `tower::CORE_MAX_HP`.
pub const CORE_LEAK_DAMAGE: f32 = 50.0;
pub const CORE_LEAK_BOSS: f32 = 200.0;

/// Enemy → Core contact (a tower-defense **leak**): an enemy that reaches the
/// Core detonates against it — the Core loses integrity and the enemy is
/// consumed (despawned with no score/drops: it was not *killed*). This is the
/// Core's analog of `enemy_contact_player`, but the Core has no shield and the
/// attacker does not survive, so there is no bounce/separation — contact ends
/// the enemy. `tower::core_lose_check` reads the depleted `Health` and ends the run.
pub fn enemy_contact_core(
    mut commands: Commands,
    mut core: Query<(&Transform, &Collider, &mut Health), With<Core>>,
    enemies: Query<(Entity, &Transform, &Collider, Has<Boss>), (With<Enemy>, Without<Core>)>,
) {
    let Ok((ctf, cc, mut chp)) = core.single_mut() else {
        return;
    };
    let cpos = ctf.translation.truncate();
    for (e, etf, ec, is_boss) in &enemies {
        let reach = cc.radius + ec.radius;
        if cpos.distance_squared(etf.translation.truncate()) >= reach * reach {
            continue; // not yet at the Core
        }
        chp.current -= if is_boss { CORE_LEAK_BOSS } else { CORE_LEAK_DAMAGE };
        // `try_despawn`: the same enemy may have been killed by a turret/commander
        // shot this very tick (it's at the Core, under fire), so a plain despawn
        // would race with `apply_damage` and warn.
        commands.entity(e).try_despawn();
    }
}

/// Enemy bullets that reach the Core chip its integrity — the *ranged* half of
/// the assault (ramming via `enemy_contact_core` is the melee half). Player/tower
/// bullets are ignored here (they're handled by `bullet_hits_enemy`).
pub fn enemy_bullet_hits_core(
    mut commands: Commands,
    mut core: Query<(&Transform, &Collider, &mut Health), With<Core>>,
    bullets: Query<(Entity, &Transform, &Collider, &Bullet)>,
) {
    let Ok((ctf, cc, mut chp)) = core.single_mut() else {
        return;
    };
    let cpos = ctf.translation.truncate();
    for (e, btf, bc, bullet) in &bullets {
        if bullet.kind != BulletKind::Enemy {
            continue;
        }
        let reach = cc.radius + bc.radius;
        if cpos.distance_squared(btf.translation.truncate()) <= reach * reach {
            chp.current -= bullet.damage;
            commands.entity(e).try_despawn();
        }
    }
}

/// Player ↔ asteroid contact: a **pure physics bounce** (no damage — rocks are
/// inert). The ship used to fly straight through asteroids; now it caroms off,
/// with the same momentum impulse as enemy contact (mass ∝ area, so a big rock
/// shoves the ship hard while barely moving itself). Runs in the collision group.
pub fn player_hits_asteroid(
    mut player: Query<(&mut Transform, &mut Velocity, &Collider), With<Ship>>,
    mut asteroids: Query<
        (&mut Transform, &mut Velocity, &Collider),
        (With<Asteroid>, Without<Ship>),
    >,
) {
    let Ok((mut ptf, mut pvel, pc)) = player.single_mut() else {
        return;
    };
    for (mut atf, mut avel, ac) in &mut asteroids {
        let ppos = ptf.translation.truncate();
        let apos = atf.translation.truncate();
        let reach = pc.radius + ac.radius;
        let delta = ppos - apos;
        let dist = delta.length();
        if dist >= reach {
            continue;
        }
        let n = if dist > 1e-4 { delta / dist } else { Vec2::Y };

        // Separate the overlap (push both out along the normal).
        let push = n * ((reach - dist) * OVERLAP_SEPARATION_RATIO);
        ptf.translation.x += push.x;
        ptf.translation.y += push.y;
        atf.translation.x -= push.x;
        atf.translation.y -= push.y;

        // Momentum impulse (elastic, mass ∝ area).
        let (mp, ma) = (pc.radius * pc.radius, ac.radius * ac.radius);
        let closing = (pvel.0 - avel.0).dot(n);
        if closing < 0.0 {
            let j = -(1.0 + CONTACT_RESTITUTION) * closing / (1.0 / mp + 1.0 / ma);
            pvel.0 += (j / mp) * n;
            avel.0 -= (j / ma) * n;
        }
        pvel.0 += n * CONTACT_POP;
        avel.0 -= n * CONTACT_POP;
    }
}
