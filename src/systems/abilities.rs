//! The 4-slot ability loadout activation (Phase AB). Ticks the per-slot
//! cooldowns and, on an ability-key press for a ready slot holding an equipped
//! ability, fires that ability's effect. Reuses dps's existing effect code
//! (Bulwark/Field Medic/Deflector Orbs/EMP Pulse) and adds Blink.
//!
//! **Key binding (transitional):** the slots fire on **Numpad 1–4** for now,
//! because Digit 1–5 already select weapons (`systems::weapons::cycle_weapon`).
//! The final binding (Digit 1–4, with weapon-select moved into the radial menu)
//! lands with the BUILD-tree / input rework in Phase UI. The legacy keybinds
//! (C/X/V/G/H/J/K) still work in parallel during the migration.

use crate::components::*;
use crate::messages::Fire;
use crate::systems::skills::{spawn_deflector_orbs, EMP_RADIUS};
use crate::systems::weapons::BASE_BULLET_SPEED;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

/// Numpad keys 1–4 → loadout slots 0–3.
const SLOT_KEYS: [KeyCode; 4] = [
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
];

/// Blink teleport distance (px) and i-frame (s) — `player/abilities.js` Blink.
const BLINK_DISTANCE: f32 = 220.0;
const BLINK_IFRAME: f32 = 0.35;

/// Gravity Snare (`weapon-data.js`): yank non-boss enemies inward, no closer than
/// `SNARE_MIN_DIST`, by `min(dist − minDist, dist × SNARE_PULL)`.
const SNARE_RADIUS: f32 = 320.0;
const SNARE_PULL: f32 = 0.6;
const SNARE_MIN_DIST: f32 = 70.0;

/// Designator (`weapon-data.js`): MARK every enemy within this radius.
const DESIGNATOR_RADIUS: f32 = 360.0;

/// Sentry Drone (`weapon-data.js` SENTRY_DRONE + `player/abilities.js`): one drone
/// orbits the ship at `SENTRY_ORBIT_RADIUS`, turning `SENTRY_ORBIT_OMEGA` rad/s,
/// auto-firing `SENTRY_DAMAGE` shots every `SENTRY_FIRE_INTERVAL` s for 8 s.
const SENTRY_ORBIT_RADIUS: f32 = 58.0;
const SENTRY_ORBIT_OMEGA: f32 = 4.0;
const SENTRY_FIRE_INTERVAL: f32 = 0.6;
const SENTRY_DAMAGE: f32 = 1.2;
const SENTRY_COUNT: u32 = 1;

/// Tick the loadout cooldowns and fire equipped abilities on their slot key.
/// A press is consumed (cooldown spent) only if the slot holds an *implemented*
/// ability that is off cooldown; empty/on-cooldown/deferred slots are no-ops.
pub fn activate_loadout(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    // Optional so headless tests run without the audio assets.
    sfx: Option<Res<crate::audio::Sfx>>,
    mut commands: Commands,
    equipped: Res<EquippedAbilities>,
    mut cds: ResMut<AbilityCooldowns>,
    mut infusion: ResMut<crate::systems::weapons::ElementInfusion>,
    mut player: Query<(Entity, &mut Transform), With<Ship>>,
    mut enemies: Query<(Entity, &mut Transform, Has<Boss>), (With<Enemy>, Without<Ship>)>,
) {
    cds.tick(time.delta_secs());

    let Ok((player_e, mut ptf)) = player.single_mut() else {
        return; // no ship (GameOver, etc.) — cooldowns still ticked above
    };
    let center = ptf.translation.truncate();
    let facing = (ptf.rotation * Vec3::Y).truncate().normalize_or_zero();

    for slot in 0..4 {
        if !keys.just_pressed(SLOT_KEYS[slot]) {
            continue;
        }
        let Some(ability) = equipped.0[slot] else {
            continue; // empty slot
        };
        if !cds.is_ready(slot) {
            continue; // still cooling down
        }

        let fired = match ability {
            Ability::Bulwark => {
                commands.entity(player_e).insert(Bulwark { seconds: 4.0 });
                true
            }
            Ability::FieldMedic => {
                commands.entity(player_e).insert(Repairing {
                    seconds: 5.0,
                    rate: 3.0,
                });
                true
            }
            Ability::DeflectorOrbs => {
                spawn_deflector_orbs(&mut commands, center);
                true
            }
            Ability::EmpPulse => {
                for (e, etf, _boss) in &enemies {
                    if etf.translation.truncate().distance(center) <= EMP_RADIUS {
                        commands.entity(e).insert(Stunned { secs: 2.0 });
                    }
                }
                true
            }
            Ability::GravitySnare => {
                // Yank non-boss enemies inward (instant position pull).
                for (_e, mut etf, is_boss) in &mut enemies {
                    if is_boss {
                        continue;
                    }
                    let to_ship = center - etf.translation.truncate();
                    let dist = to_ship.length();
                    if dist > SNARE_MIN_DIST && dist <= SNARE_RADIUS {
                        let pull = (dist - SNARE_MIN_DIST).min(dist * SNARE_PULL);
                        let step = to_ship / dist * pull;
                        etf.translation.x += step.x;
                        etf.translation.y += step.y;
                    }
                }
                true
            }
            Ability::Designator => {
                for (e, etf, _boss) in &enemies {
                    if etf.translation.truncate().distance(center) <= DESIGNATOR_RADIUS {
                        commands.entity(e).insert(Mark { secs: MARK_SECS });
                    }
                }
                true
            }
            Ability::SecondWind => {
                // Arm a one-time death save; consumed by damage::apply_damage.
                commands.entity(player_e).insert(SecondWindArmed);
                true
            }
            Ability::ElementalInfusion => {
                // Re-element shots to the next element for 8 s (spawn_bullets
                // reads ElementInfusion); each cast cycles forward.
                infusion.element = Some(crate::systems::weapons::next_infusion_element(
                    infusion.element,
                ));
                infusion.secs = 8.0;
                true
            }
            Ability::SentryDrone => {
                spawn_sentry_drones(&mut commands, center, SENTRY_COUNT, ability.duration());
                true
            }
            Ability::Blink => {
                let dest = center + facing * BLINK_DISTANCE;
                ptf.translation.x = dest.x;
                ptf.translation.y = dest.y;
                commands
                    .entity(player_e)
                    .insert(Invulnerable { seconds: BLINK_IFRAME });
                true
            }
            Ability::CryoField
            | Ability::StasisField
            | Ability::StormCell
            | Ability::PyreAura => {
                if let Some((status, radius, tick)) = ability.field_params() {
                    spawn_ability_field(
                        &mut commands,
                        center,
                        status,
                        radius,
                        ability.duration(),
                        tick,
                    );
                }
                true
            }
        };

        if fired {
            cds.trigger(slot, ability.cooldown());
            if let Some(sfx) = &sfx {
                commands.spawn((AudioPlayer::new(sfx.ability.clone()), PlaybackSettings::DESPAWN));
            }
        }
    }
}

/// Spawn a drop-zone field at `center` (the four Cryo/Stasis/Storm/Pyre
/// abilities). `timer: 0.0` so it applies its status on the very next tick.
pub fn spawn_ability_field(
    commands: &mut Commands,
    center: Vec2,
    status: FieldStatus,
    radius: f32,
    secs: f32,
    tick: f32,
) {
    commands.spawn((
        AbilityField {
            status,
            radius,
            secs,
            tick,
            timer: 0.0,
        },
        field_shape(radius, status),
        Transform::from_translation(center.extend(0.4)),
    ));
}

/// A translucent filled disc tinted to the field's element (HDR for a soft bloom).
fn field_shape(radius: f32, status: FieldStatus) -> Shape {
    let (r, g, b) = match status {
        FieldStatus::Freeze => (0.5, 0.85, 1.0),
        FieldStatus::Chill => (0.65, 0.72, 1.0),
        FieldStatus::Conduct => (1.0, 0.9, 0.3),
        FieldStatus::Burn => (1.0, 0.45, 0.15),
    };
    let mut path = ShapePath::new();
    let segments = 40;
    for i in 0..segments {
        let a = i as f32 / segments as f32 * TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::srgba(r, g, b, 0.12))
        .build()
}

/// Drop-zone fields: each tick interval, re-apply the field's status to every
/// enemy inside its radius; despawn the field when its lifetime elapses.
pub fn tick_ability_fields(
    time: Res<Time>,
    mut commands: Commands,
    mut fields: Query<(Entity, &Transform, &mut AbilityField)>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    let dt = time.delta_secs();
    for (field_e, ftf, mut field) in &mut fields {
        field.secs -= dt;
        if field.secs <= 0.0 {
            commands.entity(field_e).despawn();
            continue;
        }
        field.timer -= dt;
        if field.timer > 0.0 {
            continue;
        }
        field.timer = field.tick;

        let center = ftf.translation.truncate();
        let r2 = field.radius * field.radius;
        for (e, etf) in &enemies {
            if etf.translation.truncate().distance_squared(center) > r2 {
                continue;
            }
            match field.status {
                FieldStatus::Freeze => {
                    commands.entity(e).insert(Frozen { secs: FREEZE_SECS });
                }
                FieldStatus::Chill => {
                    commands.entity(e).insert(Chill { secs: CHILL_SECS });
                }
                FieldStatus::Conduct => {
                    commands.entity(e).insert(Conduct { secs: CONDUCT_SECS });
                }
                FieldStatus::Burn => {
                    commands.entity(e).insert(Burning { dps: 6.0, secs: 1.0 });
                }
            }
        }
    }
}

/// A small orange drone diamond (HDR for bloom).
fn sentry_shape() -> Shape {
    let r = 7.0_f32;
    let pts = [
        Vec2::new(0.0, r),
        Vec2::new(r, 0.0),
        Vec2::new(0.0, -r),
        Vec2::new(-r, 0.0),
    ];
    let mut path = ShapePath::new().move_to(pts[0]);
    for p in &pts[1..] {
        path = path.line_to(*p);
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(6.0, 2.4, 0.6))
        .build()
}

/// Spawn `count` Sentry Drones orbiting `center`, each living `secs` seconds.
pub fn spawn_sentry_drones(commands: &mut Commands, center: Vec2, count: u32, secs: f32) {
    for i in 0..count {
        let angle = i as f32 / count.max(1) as f32 * TAU;
        let pos = center + Vec2::new(angle.cos(), angle.sin()) * SENTRY_ORBIT_RADIUS;
        commands.spawn((
            Sentry {
                secs,
                angle,
                fire_timer: 0.0,
            },
            sentry_shape(),
            Transform::from_translation(pos.extend(0.6)),
        ));
    }
}

/// Sentry Drones: orbit the ship, aim at the nearest enemy, and emit a `Fire`
/// (player faction) each `SENTRY_FIRE_INTERVAL`; despawn when their lifetime ends.
pub fn tick_sentry_drones(
    time: Res<Time>,
    mut commands: Commands,
    mut fire: MessageWriter<Fire>,
    player: Query<&Transform, (With<Ship>, Without<Sentry>)>,
    mut sentries: Query<(Entity, &mut Transform, &mut Sentry), (Without<Ship>, Without<Enemy>)>,
    enemies: Query<&Transform, (With<Enemy>, Without<Sentry>)>,
) {
    let dt = time.delta_secs();
    // Without a player, still expire the drones (they orbit nothing).
    let center = player.single().ok().map(|t| t.translation.truncate());

    for (e, mut tf, mut sentry) in &mut sentries {
        sentry.secs -= dt;
        if sentry.secs <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let Some(center) = center else {
            continue;
        };
        sentry.angle += SENTRY_ORBIT_OMEGA * dt;
        let pos = center + Vec2::new(sentry.angle.cos(), sentry.angle.sin()) * SENTRY_ORBIT_RADIUS;
        tf.translation.x = pos.x;
        tf.translation.y = pos.y;

        // Nearest enemy → aim.
        let mut target = None;
        let mut best = f32::INFINITY;
        for etf in &enemies {
            let ep = etf.translation.truncate();
            let d2 = ep.distance_squared(pos);
            if d2 < best {
                best = d2;
                target = Some(ep);
            }
        }

        sentry.fire_timer -= dt;
        if let Some(target) = target {
            if sentry.fire_timer <= 0.0 {
                sentry.fire_timer = SENTRY_FIRE_INTERVAL;
                let dir = (target - pos).normalize_or_zero();
                if dir != Vec2::ZERO {
                    fire.write(Fire {
                        origin: pos,
                        dir,
                        damage: SENTRY_DAMAGE,
                        speed: BASE_BULLET_SPEED,
                        faction: Faction::Player,
                        element: crate::combat::element::Element::Kinetic,
                        homing: false,
                    });
                }
            }
        }
    }
}
