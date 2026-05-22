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

use crate::components::*;
use crate::messages::{Damage, Knockback};
use crate::resources::{roll_crit, EnergyMeter, GameRng, KillStreak, ENERGY_PER_HIT};
use crate::systems::shop::{
    explosion_radius, knock_chance, stun_chance, vampirism_frac, UpgradeId, Upgrades, KNOCK_PX,
};
use bevy::prelude::*;

pub fn bullet_hits_enemy(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut knock: MessageWriter<Knockback>,
    streak: Res<KillStreak>,
    upgrades: Res<Upgrades>,
    mut rng: ResMut<GameRng>,
    mut energy: ResMut<EnergyMeter>,
    mut bullets: Query<(Entity, &Transform, &Collider, &mut Bullet)>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
    mut player_hp: Query<&mut Health, With<Ship>>,
) {
    // Kill-streak multiplier scales all player bullet damage (spec III.6).
    let streak_mult = streak.multiplier();
    // VAMPIRISM passive: heal a fraction of damage dealt (spec III.5).
    let vamp = vampirism_frac(upgrades.owned(UpgradeId::Vampirism));
    // `_STUN` bullet trait: chance to stun the enemy on hit (spec III.2/III.6).
    let stun_p = stun_chance(upgrades.owned(UpgradeId::StunShot));
    // `_EXPLODE` bullet trait: AoE splash radius on hit (0 = off).
    let explode_r = explosion_radius(upgrades.owned(UpgradeId::ExplodeShot));
    // `_KNOCK` bullet trait: chance to shove the enemy on hit.
    let knock_p = knock_chance(upgrades.owned(UpgradeId::KnockShot));
    for (bullet_e, btf, bc, mut bullet) in &mut bullets {
        if bullet.kind != BulletKind::Player {
            continue;
        }
        for (enemy_e, etf, ec) in &enemies {
            let reach = bc.radius + ec.radius;
            let d2 = btf
                .translation
                .truncate()
                .distance_squared(etf.translation.truncate());
            if d2 <= reach * reach {
                // Streak multiplier × per-hit crit roll (spec III.6).
                let amount = bullet.damage * streak_mult * roll_crit(&mut rng);
                dmg.write(Damage {
                    target: enemy_e,
                    amount,
                });
                // VAMPIRISM: heal the player for a fraction of the damage dealt.
                if vamp > 0.0 {
                    if let Ok(mut hp) = player_hp.single_mut() {
                        hp.current = (hp.current + amount * vamp).min(hp.max);
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
                    for (e2, etf2, ec2) in &enemies {
                        if e2 == enemy_e {
                            continue;
                        }
                        if etf2.translation.truncate().distance(hit_pos) <= explode_r + ec2.radius {
                            dmg.write(Damage { target: e2, amount: splash });
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
    mut dmg: MessageWriter<Damage>,
    mut player: Query<(Entity, &mut Transform, &mut Velocity, &Collider), With<Ship>>,
    mut enemies: Query<(Entity, &mut Transform, &mut Velocity, &Collider), (With<Enemy>, Without<Ship>)>,
) {
    let Ok((player_e, mut ptf, mut pvel, pc)) = player.single_mut() else {
        return;
    };
    for (enemy_e, mut etf, mut evel, ec) in &mut enemies {
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
