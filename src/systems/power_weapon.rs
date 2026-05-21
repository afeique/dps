//! Power weapon: Homing Missile Salvo (KeyE).
//!
//! Fires a spread of 3 homing missiles that curve toward the nearest enemy.
//! Each missile is a `Bullet { kind: BulletKind::Player }` so the existing
//! `bullet_hits_enemy` collision system handles damage and despawning without
//! any modification.
//!
//! Public surface for the parent (`app.rs`) to register:
//! - **Component** `Homing`
//! - **Resource**  `PowerWeaponCooldown` (init via `init_resource`)
//! - **System**    `fire_power_weapon` (Update)
//! - **System**    `homing_steer`      (Update, after `fire_power_weapon`)

use crate::components::*;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ─── Component ───────────────────────────────────────────────────────────────

/// Marks a missile entity; carries the maximum angular turn rate so the
/// steering system can read it without a separate resource.
#[derive(Component)]
pub struct Homing {
    /// Maximum angular correction per second, in radians.
    pub turn_rate: f32,
}

// ─── Resource ────────────────────────────────────────────────────────────────

/// Countdown timer (seconds) before the player may fire another salvo.
/// Starts at 0 so the weapon is immediately available on the first frame.
#[derive(Resource, Default)]
pub struct PowerWeaponCooldown(pub f32);

// ─── Shape ───────────────────────────────────────────────────────────────────

/// A sharp diamond / dart silhouette for the missile.
///
/// Authored nose-up (+Y forward) to match the ship convention.  The four
/// vertices form an asymmetric dart: a long nose above the origin and a short
/// tail below, with narrow lateral fins.  HDR-emissive hot orange so Bloom
/// gives it a fiery glow.
pub fn shape() -> Shape {
    // Vertices in Bevy space (+Y up, nose at the top).
    //   nose   → (  0,  +8 )
    //   right  → ( +3,  -2 )
    //   tail   → (  0,  -5 )
    //   left   → ( -3,  -2 )
    let nose  = Vec2::new( 0.0,  8.0);
    let right = Vec2::new( 3.0, -2.0);
    let tail  = Vec2::new( 0.0, -5.0);
    let left  = Vec2::new(-3.0, -2.0);

    let path = ShapePath::new()
        .move_to(nose)
        .line_to(right)
        .line_to(tail)
        .line_to(left)
        .close();

    ShapeBuilder::with(&path)
        // Dark charcoal body so the emissive edge pop stands out.
        .fill(Color::linear_rgb(0.08, 0.02, 0.0))
        // Hot orange — over-bright so Bloom turns it into a fire halo.
        .stroke((Color::linear_rgb(9.0, 3.0, 0.5), 1.5))
        .build()
}

// ─── Helper ──────────────────────────────────────────────────────────────────

/// Rotate a `Vec2` counter-clockwise by `angle` radians.
#[inline]
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Steer `current` velocity toward `desired` direction by at most `max_angle`
/// radians, preserving the original speed.
///
/// Returns the new velocity vector.
#[inline]
fn steer_toward(current: Vec2, desired_dir: Vec2, max_angle: f32) -> Vec2 {
    let speed = current.length();
    if speed < 1e-6 {
        return current;
    }
    let cur_dir = current / speed;

    // Signed angle from cur_dir → desired_dir  (positive = CCW in Bevy +Y-up).
    let angle = {
        let cross = cur_dir.x * desired_dir.y - cur_dir.y * desired_dir.x;
        let dot   = cur_dir.dot(desired_dir);
        cross.atan2(dot)
    };

    let clamped = angle.clamp(-max_angle, max_angle);
    rotate(cur_dir, clamped) * speed
}

// ─── Systems ─────────────────────────────────────────────────────────────────

/// Tick the cooldown and fire a 3-missile salvo when **KeyE** is pressed.
///
/// The salvo is spread across −25°, 0°, and +25° relative to the ship's
/// forward vector.  Each missile is spawned with the `Homing` component so
/// `homing_steer` will curve it toward the nearest enemy each frame.
pub fn fire_power_weapon(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cd: ResMut<PowerWeaponCooldown>,
    mut commands: Commands,
    player: Query<&Transform, With<Ship>>,
) {
    let dt = time.delta_secs();

    // Tick down; clamp so we never go negative.
    cd.0 = (cd.0 - dt).max(0.0);

    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    if cd.0 > 0.0 {
        return;
    }

    let Ok(tf) = player.single() else {
        return;
    };

    // Forward direction in world space (+Y is "up" in Bevy 2D).
    let fwd = (tf.rotation * Vec3::Y).truncate().normalize_or_zero();
    // Muzzle position: 22 px ahead of the ship center (ship nose).
    let nose = tf.translation.truncate() + fwd * 22.0;

    // Spread angles for the three missiles (radians).
    const SPREAD: f32 = 25.0 * std::f32::consts::PI / 180.0; // 25°
    let offsets = [-SPREAD, 0.0, SPREAD];

    for offset in offsets {
        let dir = rotate(fwd, offset);
        // Missile flight speed (world units / second).
        const SPEED: f32 = 420.0;

        commands.spawn((
            Bullet { kind: BulletKind::Player, damage: 22.0 },
            Velocity(dir * SPEED),
            Collider { radius: 5.0 },
            Faction::Player,
            Homing { turn_rate: 4.0 },
            Lifetime { seconds: 2.5 },
            shape(),
            Transform::from_translation(nose.extend(1.0)),
        ));
    }

    // Start the cooldown after a successful salvo.
    cd.0 = 0.8;
}

/// Curve each homing missile's velocity toward the nearest living enemy.
///
/// The correction is clamped to `turn_rate * dt` radians per tick so the
/// missile arcs smoothly rather than teleporting direction.  If no enemies
/// exist the missile flies straight (velocity unchanged).
pub fn homing_steer(
    time: Res<Time>,
    enemies: Query<&Transform, With<Enemy>>,
    mut missiles: Query<(&mut Velocity, &Transform, &Homing)>,
) {
    let dt = time.delta_secs();

    for (mut vel, mtf, homing) in &mut missiles {
        let origin = mtf.translation.truncate();

        // Find the nearest enemy by squared distance (avoids a sqrt per check).
        let target = enemies
            .iter()
            .map(|etf| etf.translation.truncate())
            .min_by(|a, b| {
                let da = a.distance_squared(origin);
                let db = b.distance_squared(origin);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });

        let Some(target_pos) = target else {
            // No enemies — fly straight.
            continue;
        };

        let to_target = (target_pos - origin).normalize_or_zero();
        if to_target == Vec2::ZERO {
            continue;
        }

        let max_turn = homing.turn_rate * dt;
        vel.0 = steer_toward(vel.0, to_target, max_turn);
    }
}
