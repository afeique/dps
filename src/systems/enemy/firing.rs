//! Per-kind enemy firing patterns ported from
//! `js/modules/enemy/firing.js` and `js/modules/enemy/enemy-data.js`.
//!
//! Each `EnemyKind` maps to its JS `shootPattern` entry:
//!   Hunter     → `hunter_single`    (3-shot burst → single aimed per tick)
//!   Guardian   → `guardian_spread`  (5-bullet fan; simplified to 3 here)
//!   Wasp       → `wasp_machinegun`  (rapid aimed shot with aim jitter)
//!   Stalker    → `charged_laser`    (fast heavy single aimed shot)
//!   Drifter    → `arc_lightning`    (5-shot wide arc; lightning bolt → bullets)
//!   Prowler    → `missile`          (1 slow aimed shot; missile → bullet)
//!   Weaver     → `spiral_laser`     (2 opposing spiral shots keyed to time)
//!   Sentinel   → `sentinel_sweep`   (3-shot fan with sinusoidal sweep offset)
//!   Tangerine  → `lay_mine`         (1 very slow shot dropped toward player)
//!   Titan      → `sweep_laser`      (5-shot wide fan with sin-sweep; laser → bullets)
//!
//! Simplifications vs. JS 5.80.x:
//!   • Lasers (Stalker `charged_laser`, Titan `sweep_laser`) approximated as
//!     fast/wide bullet salvos — no multi-segment beam geometry or screen shake.
//!   • Missiles (Prowler `missile`) approximated as slow aimed bullets —
//!     no homing or acceleration.
//!   • Mines (Tangerine `lay_mine`) approximated as very slow aimed bullets —
//!     no persistent `homing_mine` movement pattern, no health.
//!   • Arc-lightning (Drifter) is a 5-shot spread — no fractal bolt rendering.
//!   • No burst state machine (3-shot sub-bursts) — the `FireCooldown` already
//!     provides the inter-volley cadence; each cooldown expiry emits a full volley.
//!   • No aim jitter scaling, no campaign speed multiplier.
//!
//! Per-bullet damage is the spec IV.3 `LS(n)` base value at level 1 (in-run
//! leveling is retired, so `LS(n) = n`): Wasp 1; Hunter/Guardian/Drifter/Weaver/
//! Sentinel 2; Stalker/Prowler/Titan 3; Tangerine 4. These are small by design —
//! the player has only 40 HP (spec II.2), so a few hits matter.

use crate::components::{Enemy, EnemyKind, Faction, FireCooldown, Frozen, Raged, Ship, Stunned};
use crate::messages::Fire;
use bevy::prelude::*;

// ── Helper ────────────────────────────────────────────────────────────────────

/// Rotate a unit (or any) `Vec2` by `angle` radians counter-clockwise.
#[inline]
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

// ── System ────────────────────────────────────────────────────────────────────

/// Tick enemy fire cooldowns; on expiry emit a per-kind bullet pattern.
///
/// Replaces the generic `enemy_fire` system (one aimed shot for any enemy).
/// Each `EnemyKind` now emits a pattern faithful to `firing.js`.
pub fn enemy_firing(
    time: Res<Time>,
    mut fire: MessageWriter<Fire>,
    player: Query<&Transform, With<Ship>>,
    // Stunned OR frozen enemies can't fire (spec III.6 / E3) — their cooldown
    // simply freezes. `Has<Raged>` makes a raged boss's bullets home (spec IV.7).
    mut q: Query<
        (&Enemy, &Transform, &mut FireCooldown, Has<Raged>),
        (Without<Stunned>, Without<Frozen>),
    >,
) {
    let Ok(player_tf) = player.single() else { return };
    let player_pos = player_tf.translation.truncate();
    let dt = time.delta_secs();
    let t = time.elapsed_secs();

    for (enemy, tf, mut fc, raged) in &mut q {
        // Tick cooldown.
        fc.timer = (fc.timer - dt).max(0.0);
        if fc.timer > 0.0 {
            continue;
        }

        let enemy_pos = tf.translation.truncate();
        let aim = player_pos - enemy_pos;
        if aim == Vec2::ZERO {
            // On top of the player — skip until there is separation.
            continue;
        }
        let aim_dir = aim.normalize();

        // Reset cooldown before pattern emit (patterns may differ in shape but
        // all use the same cooldown field).
        fc.timer = fc.cooldown;

        match enemy.kind {
            // ── Hunter — hunter_single ────────────────────────────────────────
            // JS: 3-shot burst (75 ms spacing) fired in `faceAngle` direction.
            // Approximation: single aimed shot each cooldown expiry; the
            // `FireCooldown` already models the between-burst gap (0.6–2.2 s).
            EnemyKind::Hunter => {
                fire.write(Fire {
                    origin: enemy_pos + aim_dir * 22.0,
                    dir: aim_dir,
                    damage: 2.0, // shootBurst3 → default bullet dmg (spec IV.5)
                    speed: 340.0,
                    faction: Faction::Enemy,
                    homing: raged,
                });
            }

            // ── Guardian — guardian_spread ────────────────────────────────────
            // JS: 5-bullet fan at ±0.5, ±0.25, 0 rad offsets from `faceAngle`.
            // Simplified to 3 shots (±18° ≈ 0.314 rad and center) to stay cheap.
            EnemyKind::Guardian => {
                for offset in [-0.314_f32, 0.0, 0.314] {
                    let dir = rotate(aim_dir, offset);
                    fire.write(Fire {
                        origin: enemy_pos + dir * 22.0,
                        dir,
                        damage: 2.0, // guardian_spread → LS(2) (spec IV.3)
                        speed: 300.0,
                        faction: Faction::Enemy,
                        homing: raged,
                    });
                }
            }

            // ── Wasp — wasp_machinegun ────────────────────────────────────────
            // JS: rapid single shot with slight aim jitter (±5° ≈ ±0.087 rad).
            // The short `FireCooldown` (0.45–2 s) provides the machine-gun cadence.
            EnemyKind::Wasp => {
                // Jitter derived deterministically from time so no rand dependency.
                let jitter = (t * 31.7).sin() * 0.087;
                let dir = rotate(aim_dir, jitter);
                fire.write(Fire {
                    origin: enemy_pos + dir * 22.0,
                    dir,
                    damage: 1.0, // machinegun → LS(1) (spec IV.3)
                    speed: 360.0,
                    faction: Faction::Enemy,
                    homing: raged,
                });
            }

            // ── Stalker — charged_laser ───────────────────────────────────────
            // JS: charges for ~1.5 s then fires a close-range laser beam (8
            // segmented bullets). Approximated as one fast heavy aimed shot.
            EnemyKind::Stalker => {
                fire.write(Fire {
                    origin: enemy_pos + aim_dir * 22.0,
                    dir: aim_dir,
                    damage: 3.0, // chargedLaser → LS(3) (spec IV.3)
                    speed: 420.0,
                    faction: Faction::Enemy,
                    homing: raged,
                });
            }

            // ── Drifter — arc_lightning ───────────────────────────────────────
            // JS: charges then fires a fractal lightning bolt (damage orbs along
            // the spine). Approximated as a 5-shot wide arc (±40° total).
            EnemyKind::Drifter => {
                // 5 shots evenly spaced across ±40° (≈ ±0.698 rad).
                let half_arc = 0.698_f32; // 40°
                let steps = 4_u32; // indices 0..=4
                for i in 0..=steps {
                    let frac = i as f32 / steps as f32; // 0.0 → 1.0
                    let offset = -half_arc + frac * (half_arc * 2.0);
                    let dir = rotate(aim_dir, offset);
                    fire.write(Fire {
                        origin: enemy_pos + dir * 22.0,
                        dir,
                        damage: 2.0, // arcLightning → LS(2) (spec IV.3)
                        speed: 280.0,
                        faction: Faction::Enemy,
                        homing: raged,
                    });
                }
            }

            // ── Prowler — missile ─────────────────────────────────────────────
            // JS: fires a slow missile (`missile_fast_slow` pattern, initial
            // speed px/s ≈ 12 × 60 = 720; decelerated to ~150 over lifetime).
            // Approximated as a single slow aimed bullet.
            EnemyKind::Prowler => {
                fire.write(Fire {
                    origin: enemy_pos + aim_dir * 22.0,
                    dir: aim_dir,
                    damage: 3.0, // shootMissile → LS(3) (spec IV.3)
                    speed: 250.0,
                    faction: Faction::Enemy,
                    homing: raged,
                });
            }

            // ── Weaver — spiral_laser ─────────────────────────────────────────
            // JS: fires along `faceAngle` (spinning during its arc phase). Here
            // there is no face-angle component, so we derive the current spiral
            // angle from `time.elapsed_secs()` to get per-entity rotation over
            // time, then emit two opposing shots (0° + 180°) for a classic
            // double-arm spiral feel.
            EnemyKind::Weaver => {
                // Advance angle at ~1 rad/s, staggered by entity world position.
                let phase_offset = (enemy_pos.x * 0.01 + enemy_pos.y * 0.007).abs();
                let spiral_angle = t * 1.0 + phase_offset;
                let dir_a = Vec2::new(spiral_angle.cos(), spiral_angle.sin());
                let dir_b = -dir_a; // opposing arm
                for dir in [dir_a, dir_b] {
                    fire.write(Fire {
                        origin: enemy_pos + dir * 22.0,
                        dir,
                        damage: 2.0, // spiral_laser → LS(2) (spec IV.3)
                        speed: 300.0,
                        faction: Faction::Enemy,
                        homing: raged,
                    });
                }
            }

            // ── Sentinel — sentinel_sweep ─────────────────────────────────────
            // JS: `shootCircleBurst` fires 8 evenly-spaced shots in 360° while
            // the Sentinel spins up. Approximated as a 3-shot aimed fan whose
            // center sweeps sinusoidally (±20° ≈ ±0.349 rad) to convey sweep.
            EnemyKind::Sentinel => {
                let sweep = (t * 1.2).sin() * 0.349; // ±20° sweep
                for offset in [-0.314_f32, 0.0, 0.314] {
                    let dir = rotate(aim_dir, sweep + offset);
                    fire.write(Fire {
                        origin: enemy_pos + dir * 22.0,
                        dir,
                        damage: 2.0, // sentinel_sweep → LS(2) (spec IV.3)
                        speed: 290.0,
                        faction: Faction::Enemy,
                        homing: raged,
                    });
                }
            }

            // ── Tangerine — lay_mine ──────────────────────────────────────────
            // JS: spawns a `homing_mine` that slowly crawls toward the player
            // (60-second lifetime). Approximated as a very slow aimed bullet.
            EnemyKind::Tangerine => {
                fire.write(Fire {
                    origin: enemy_pos + aim_dir * 22.0,
                    dir: aim_dir,
                    damage: 4.0, // layMine → LS(4) (spec IV.3)
                    speed: 120.0,
                    faction: Faction::Enemy,
                    homing: raged,
                });
            }

            // ── Titan — sweep_laser ───────────────────────────────────────────
            // JS: tank-turret sweep laser covering ±20° around the player
            // bearing, using ease-in-out over 1.6 s. Approximated as a 5-shot
            // wide fan (±40°) whose center sweeps with `sin(t)`.
            EnemyKind::Titan => {
                let sweep = (t * 0.9).sin() * 0.349; // ±20° slow sweep
                let half_fan = 0.698_f32; // ±40° fan
                let steps = 4_u32;
                for i in 0..=steps {
                    let frac = i as f32 / steps as f32;
                    let offset = -half_fan + frac * (half_fan * 2.0);
                    let dir = rotate(aim_dir, sweep + offset);
                    fire.write(Fire {
                        origin: enemy_pos + dir * 22.0,
                        dir,
                        damage: 3.0, // sweep_laser → LS(3) (spec IV.3)
                        speed: 310.0,
                        faction: Faction::Enemy,
                        homing: raged,
                    });
                }
            }
        }
    }
}
