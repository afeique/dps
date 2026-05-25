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
use crate::resources::{crit_chance, roll_crit, EnergyMeter, GameRng, KillStreak, ENERGY_PER_HIT};
use crate::systems::items::{AffixKind, Equipment};
use crate::systems::shop::{
    executioner_bonus, explosion_radius, knock_chance, stun_chance, vampirism_frac, UpgradeId,
    Upgrades, EXECUTE_THRESHOLD, KNOCK_PX,
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

pub fn bullet_hits_enemy(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut knock: MessageWriter<Knockback>,
    mut crits: MessageWriter<crate::messages::Crit>,
    streak: Res<KillStreak>,
    upgrades: Res<Upgrades>,
    equipment: Res<Equipment>,
    mut rng: ResMut<GameRng>,
    mut energy: ResMut<EnergyMeter>,
    mut bullets: Query<(Entity, &Transform, &Collider, &mut Bullet, Option<&BulletElements>)>,
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
    // Kill-streak multiplier scales all player bullet damage (spec III.6).
    let streak_mult = streak.multiplier();
    // VAMPIRISM passive: heal a fraction of damage dealt (spec III.5) — shop
    // stacks + equipped item affixes.
    let vamp = vampirism_frac(upgrades.owned(UpgradeId::Vampirism))
        + equipment.affix_total(AffixKind::Vampirism) / 100.0;
    // Crit chance/damage scale with their upgrade stacks (spec III.6) + equipped
    // item affixes (chance as a fraction; damage as an additive bonus on the cap).
    let crit_p =
        crit_chance(upgrades.owned(UpgradeId::CritChance)) + equipment.affix_total(AffixKind::CritChance) / 100.0;
    let crit_dmg_stacks = upgrades.owned(UpgradeId::CritDamage);
    let crit_dmg_bonus = equipment.affix_total(AffixKind::CritDamage) / 100.0;
    // `_STUN` bullet trait: chance to stun the enemy on hit (spec III.2/III.6).
    let stun_p = stun_chance(upgrades.owned(UpgradeId::StunShot));
    // `_EXPLODE` bullet trait: AoE splash radius on hit (0 = off).
    let explode_r = explosion_radius(upgrades.owned(UpgradeId::ExplodeShot));
    // `_KNOCK` bullet trait: chance to shove the enemy on hit.
    let knock_p = knock_chance(upgrades.owned(UpgradeId::KnockShot));
    // EXECUTIONER passive: bonus damage vs enemies below the execute threshold.
    let exec_bonus = executioner_bonus(upgrades.owned(UpgradeId::Executioner));
    for (bullet_e, btf, bc, mut bullet, belems) in &mut bullets {
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
        ) in &enemies
        {
            let reach = bc.radius + ec.radius;
            let d2 = btf
                .translation
                .truncate()
                .distance_squared(etf.translation.truncate());
            if d2 <= reach * reach {
                // Streak multiplier × per-hit crit roll (spec III.6).
                let crit_mult = roll_crit(&mut rng, crit_p, crit_dmg_stacks, crit_dmg_bonus);
                if crit_mult > 1.0 {
                    crits.write(crate::messages::Crit); // feeds the precision mission
                }
                // EXECUTIONER: extra damage vs an enemy already below 25% HP.
                let exec = if exec_bonus > 0.0 && ehp.current < ehp.max * EXECUTE_THRESHOLD {
                    1.0 + exec_bonus
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
                    bullet.damage * streak_mult * crit_mult * exec * resist_mult * ally,
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
                // VAMPIRISM: heal the player for a fraction of the damage dealt.
                // (Over-fill allowed; overheal_to_tanks converts the overflow.)
                if vamp > 0.0 {
                    if let Ok(mut hp) = player_hp.single_mut() {
                        hp.current += amount * vamp;
                    }
                }
                // Landing a hit charges the power-weapon energy meter (spec III.3).
                energy.gain(ENERGY_PER_HIT);
                // `_STUN` trait proc: briefly stun the enemy (spec III.6).
                if stun_p > 0.0 && rng.next_f32() < stun_p {
                    commands.entity(enemy_e).insert(Stunned { secs: 1.0 });
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
                // `_EXPLODE` trait: splash the streak-scaled (no-crit) bullet
                // damage to every other enemy within the blast radius.
                if explode_r > 0.0 {
                    let hit_pos = etf.translation.truncate();
                    let splash = bullet.damage * streak_mult;
                    for (e2, etf2, ec2, _ehp2, eres2, _, _, _, _, _, _, _, _) in &enemies {
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
                // Piercing bullets pass through; others die on the first hit.
                // (One hit per frame — a fast bullet clears an enemy's radius
                // before the next tick, so re-hits are rare. Tracking a hit-set
                // for slow piercers is a later refinement.)
                if bullet.pierce == 0 {
                    commands.entity(bullet_e).despawn();
                } else {
                    bullet.pierce -= 1;
                }
                break;
            }
        }
    }
}

pub fn enemy_bullet_hits_player(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    player: Query<(Entity, &Transform, &Collider), With<Ship>>,
    bullets: Query<(Entity, &Transform, &Collider, &Bullet)>,
) {
    let Ok((player_e, ptf, pc)) = player.single() else {
        return;
    };
    for (bullet_e, btf, bc, bullet) in &bullets {
        if bullet.kind != BulletKind::Enemy {
            continue;
        }
        let reach = bc.radius + pc.radius;
        let d2 = btf
            .translation
            .truncate()
            .distance_squared(ptf.translation.truncate());
        if d2 <= reach * reach {
            dmg.write(Damage {
                target: player_e,
                amount: bullet.damage,
            });
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
/// Velocity kick driving the two bodies apart on contact (juice; the JS uses a
/// full momentum impulse — `BOUNCE_RESTITUTION 0.9`). It persists because
/// `ship_control` accumulates velocity rather than overwriting it.
const CONTACT_BOUNCE: f32 = 220.0;

/// Player ↔ enemy contact (spec III.6): the player takes 25, the enemy takes 5,
/// and both are physically separated + bounced apart. There is **no** post-hit
/// invulnerability (spec II.2) — the separation is what stops a single overlap
/// from melting the player over consecutive ticks.
pub fn enemy_contact_player(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut player: Query<(Entity, &mut Transform, &mut Velocity, &Collider, Option<&PlayerCorrode>), With<Ship>>,
    mut enemies: Query<(Entity, &Enemy, &mut Transform, &mut Velocity, &Collider), (With<Enemy>, Without<Ship>)>,
) {
    let Ok((player_e, mut ptf, mut pvel, pc, pcorrode)) = player.single_mut() else {
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
        // a weak enemy may die to the 5-dmg ram).
        dmg.write(Damage { target: player_e, amount: CONTACT_DAMAGE });
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

        // Velocity bounce apart (juice).
        pvel.0 += n * CONTACT_BOUNCE;
        evel.0 -= n * CONTACT_BOUNCE;
    }
}
