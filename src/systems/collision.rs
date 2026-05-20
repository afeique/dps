//! Collision. Phase 1 implements only the pair that closes the slice's loop:
//! player bullets → enemies, naive O(bullets × enemies). It emits a `Damage`
//! message and despawns the bullet on contact.
//!
//! Phase 3 replaces this with the ported spatial-grid broadphase
//! (`js/modules/performance/spatial-grid.js` + `combat/collision-system.js`)
//! and adds the other pairs (enemy bullets → player, AOE rings, asteroids).

use crate::components::*;
use crate::messages::Damage;
use bevy::prelude::*;

pub fn bullet_hits_enemy(
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    bullets: Query<(Entity, &Transform, &Collider, &Bullet)>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
) {
    for (bullet_e, btf, bc, bullet) in &bullets {
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
                dmg.write(Damage {
                    target: enemy_e,
                    amount: bullet.damage,
                });
                commands.entity(bullet_e).despawn();
                break; // one bullet, one hit
            }
        }
    }
}
