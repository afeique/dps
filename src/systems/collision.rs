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
use crate::messages::Damage;
use crate::resources::{roll_crit, EnergyMeter, GameRng, KillStreak, ENERGY_PER_HIT};
use bevy::prelude::*;

pub fn bullet_hits_enemy(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    streak: Res<KillStreak>,
    mut rng: ResMut<GameRng>,
    mut energy: ResMut<EnergyMeter>,
    mut bullets: Query<(Entity, &Transform, &Collider, &mut Bullet)>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
) {
    // Kill-streak multiplier scales all player bullet damage (spec III.6).
    let streak_mult = streak.multiplier();
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
                // Landing a hit charges the power-weapon energy meter (spec III.3).
                energy.gain(ENERGY_PER_HIT);
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

const CONTACT_DAMAGE: f32 = 20.0;

pub fn enemy_contact_player(
    mut dmg: MessageWriter<Damage>,
    player: Query<(Entity, &Transform, &Collider), With<Ship>>,
    enemies: Query<(&Transform, &Collider), With<Enemy>>,
) {
    let Ok((player_e, ptf, pc)) = player.single() else {
        return;
    };
    for (etf, ec) in &enemies {
        let reach = ec.radius + pc.radius;
        let d2 = etf
            .translation
            .truncate()
            .distance_squared(ptf.translation.truncate());
        if d2 <= reach * reach {
            dmg.write(Damage {
                target: player_e,
                amount: CONTACT_DAMAGE,
            });
        }
    }
}
