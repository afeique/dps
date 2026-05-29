//! Per-enemy modules (mirrors `js/modules/enemy/*`). Each kind module exposes
//! `shape()` (lyon silhouette), `stats()` (base stats from `enemy-data.js`),
//! and `ai()` (a FixedUpdate movement system filtering its own kind). `spawn`
//! dispatches on `EnemyKind` to build the entity. Generic aimed firing lives in
//! `systems::enemy_fire`; per-kind fire patterns are a later refinement.
//!
//! Ported so far: Drifter (in `render::shapes` + `systems::enemy_ai`), Hunter,
//! Guardian, Wasp. Stalker / Prowler / Weaver / Sentinel / Tangerine / Titan
//! are subsequent increments; they fall back to the drifter visual for now.

pub mod firing;
pub mod guardian;
pub mod mechanics;
pub mod hunter;
pub mod prowler;
pub mod sentinel;
pub mod stalker;
pub mod tangerine;
pub mod titan;
pub mod wasp;
pub mod weaver;

use crate::combat::element::{Element, Resistances};
use crate::components::*;
use crate::messages::{Death, Fire};
use crate::render::shapes;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// Base stats for an enemy kind (mapped from `enemy-data.js`). `speed` is in
/// world-units/second; `fire_cooldown` is `Some(seconds)` if the kind fires.
#[derive(Clone, Copy)]
pub struct EnemyStats {
    pub health: f32,
    pub radius: f32,
    pub speed: f32,
    pub fire_cooldown: Option<f32>,
}

fn shape_for(kind: EnemyKind) -> Shape {
    match kind {
        EnemyKind::Hunter => hunter::shape(),
        EnemyKind::Guardian => guardian::shape(),
        EnemyKind::Wasp => wasp::shape(),
        EnemyKind::Stalker => stalker::shape(),
        EnemyKind::Prowler => prowler::shape(),
        EnemyKind::Weaver => weaver::shape(),
        EnemyKind::Sentinel => sentinel::shape(),
        EnemyKind::Tangerine => tangerine::shape(),
        EnemyKind::Titan => titan::shape(),
        EnemyKind::Drifter => shapes::drifter_star(18.0),
        // EN: new elemental enemies reuse the silhouette of the pattern they
        // borrow (unique `cinder_ember`/`ice_crystal`/… land in an EN follow-up).
        EnemyKind::Cinder => wasp::shape(),
        EnemyKind::Glacier => guardian::shape(),
        EnemyKind::FrostLance => stalker::shape(),
        EnemyKind::AshenDetonator => hunter::shape(),
        // EN E8c — reuse the move-pattern source's silhouette.
        EnemyKind::TeslaWraith => hunter::shape(),
        EnemyKind::Plaguebearer | EnemyKind::Hydra => tangerine::shape(),
        EnemyKind::SporeCarrier => prowler::shape(),
        // EN E8d/e.
        EnemyKind::Warden => prowler::shape(),
        EnemyKind::LumenDrone => hunter::shape(),
    }
}

/// Per-kind elemental resistance map (ported 1:1 from `enemy-data.js`
/// `ENEMY_RESISTS`, :631-650): `>0` resists, `<0` is a weakness, `1` immune.
/// Hunter is neutral (empty). Stamped on every enemy at spawn; read by
/// `collision::bullet_hits_enemy` (E2). The Titan's "tanky all-around 0.30"
/// stands in until its rotating weak-core behavior lands.
pub fn resistances_for(kind: EnemyKind) -> Resistances {
    use Element::*;
    let r = Resistances::new();
    match kind {
        EnemyKind::Guardian => r.with(Kinetic, 0.30).with(Volt, -0.40),
        EnemyKind::Wasp => r.with(Cryo, -0.50),
        EnemyKind::Stalker => r.with(Radiant, 0.50).with(Void, -0.40),
        EnemyKind::Drifter => r.with(Volt, 0.60).with(Toxic, -0.40),
        EnemyKind::Prowler => r.with(Cryo, 0.40).with(Pyro, -0.50),
        EnemyKind::Weaver => r.with(Cryo, -0.40),
        EnemyKind::Sentinel => r.with(Radiant, 0.50).with(Kinetic, -0.30),
        EnemyKind::Tangerine => r.with(Pyro, 0.60).with(Cryo, -0.40),
        EnemyKind::Titan => r
            .with(Kinetic, 0.30)
            .with(Pyro, 0.30)
            .with(Cryo, 0.30)
            .with(Volt, 0.30)
            .with(Toxic, 0.30)
            .with(Void, 0.30)
            .with(Radiant, 0.30),
        EnemyKind::Hunter => r, // neutral
        // EN batch E8b resist maps (enemy-data.js:641-644).
        EnemyKind::Cinder => r.with(Pyro, 0.85).with(Cryo, -0.50),
        EnemyKind::Glacier => r.with(Cryo, 0.90).with(Pyro, -0.50),
        EnemyKind::FrostLance => r.with(Cryo, 0.40).with(Toxic, -0.40),
        EnemyKind::AshenDetonator => r.with(Pyro, 0.50).with(Cryo, -0.40),
        // EN batch E8c resist maps (enemy-data.js:645-647).
        EnemyKind::TeslaWraith => r.with(Volt, 0.85).with(Toxic, -0.50),
        EnemyKind::Plaguebearer => r.with(Toxic, 0.60).with(Radiant, -0.40),
        EnemyKind::SporeCarrier => r.with(Toxic, 0.50).with(Radiant, -0.40),
        EnemyKind::Hydra => r, // KINETIC bruiser, neutral resists
        // EN batch E8d/e (enemy-data.js:648). Warden starts neutral — its resist
        // is learned at runtime (adaptive); Lumen is radiant-tough, void-weak.
        EnemyKind::Warden => r,
        EnemyKind::LumenDrone => r.with(Radiant, 0.50).with(Void, -0.40),
    }
}

/// Per-kind **attack** element (ported from `enemy-data.js` `ENEMY_ELEMENTS`):
/// the element an enemy's shots / ram carry, driving the player's elemental
/// status (E5) + (later) the player's elemental resistance. Most of the original
/// ten are KINETIC; Stalker/Weaver/Sentinel = Radiant, Drifter = Volt, Tangerine
/// = Pyro. (The new elemental roster fills in Pyro/Cryo/Volt/Toxic in EN.)
pub fn element_for(kind: EnemyKind) -> Element {
    match kind {
        EnemyKind::Stalker | EnemyKind::Weaver | EnemyKind::Sentinel => Element::Radiant,
        EnemyKind::Drifter => Element::Volt,
        EnemyKind::Tangerine => Element::Pyro,
        EnemyKind::Hunter
        | EnemyKind::Guardian
        | EnemyKind::Wasp
        | EnemyKind::Prowler
        | EnemyKind::Titan => Element::Kinetic,
        // EN batch E8b attack elements (enemy-data.js:616-619).
        EnemyKind::Cinder | EnemyKind::AshenDetonator => Element::Pyro,
        EnemyKind::Glacier | EnemyKind::FrostLance => Element::Cryo,
        // EN batch E8c attack elements (enemy-data.js:620-622).
        EnemyKind::TeslaWraith => Element::Volt,
        EnemyKind::Plaguebearer | EnemyKind::SporeCarrier => Element::Toxic,
        EnemyKind::Hydra | EnemyKind::Warden => Element::Kinetic,
        EnemyKind::LumenDrone => Element::Radiant,
    }
}

fn stats_for(kind: EnemyKind) -> EnemyStats {
    match kind {
        EnemyKind::Hunter => hunter::stats(),
        EnemyKind::Guardian => guardian::stats(),
        EnemyKind::Wasp => wasp::stats(),
        EnemyKind::Stalker => stalker::stats(),
        EnemyKind::Prowler => prowler::stats(),
        EnemyKind::Weaver => weaver::stats(),
        EnemyKind::Sentinel => sentinel::stats(),
        EnemyKind::Tangerine => tangerine::stats(),
        EnemyKind::Titan => titan::stats(),
        EnemyKind::Drifter => EnemyStats {
            // Spec roster: Drifter HP 9, radius 38 (the lyon silhouette is built
            // at 18 px; collider matches the original visual for now).
            health: 9.0,
            radius: 18.0,
            speed: 60.0,
            fire_cooldown: Some(1.4),
        },
        // EN batch E8b (enemy-data.js:392-465). Speed is px/frame×60; the borrowed
        // AI uses the source kind's hardcoded speed, so these tune HP/size/element.
        EnemyKind::Cinder => EnemyStats {
            health: 4.0,
            radius: 15.0,
            speed: 198.0,
            fire_cooldown: Some(0.7),
        },
        EnemyKind::Glacier => EnemyStats {
            health: 18.0,
            radius: 25.0,
            speed: 60.0,
            fire_cooldown: Some(3.0),
        },
        EnemyKind::FrostLance => EnemyStats {
            health: 7.0,
            radius: 19.0,
            speed: 180.0,
            fire_cooldown: Some(1.4),
        },
        EnemyKind::AshenDetonator => EnemyStats {
            health: 8.0,
            radius: 18.0,
            speed: 132.0,
            fire_cooldown: Some(1.0),
        },
        // EN batch E8c (enemy-data.js:473-577).
        EnemyKind::TeslaWraith => EnemyStats {
            health: 6.0,
            radius: 17.0,
            speed: 192.0,
            fire_cooldown: Some(1.4),
        },
        EnemyKind::Plaguebearer => EnemyStats {
            health: 11.0,
            radius: 22.0,
            speed: 120.0,
            fire_cooldown: Some(2.0),
        },
        EnemyKind::SporeCarrier => EnemyStats {
            health: 13.0,
            radius: 23.0,
            speed: 66.0,
            fire_cooldown: Some(1.2),
        },
        EnemyKind::Hydra => EnemyStats {
            health: 14.0,
            radius: 22.0,
            speed: 84.0,
            fire_cooldown: Some(1.2),
        },
        // EN batch E8d/e (enemy-data.js:518-599).
        EnemyKind::Warden => EnemyStats {
            health: 16.0,
            radius: 23.0,
            speed: 51.0,
            fire_cooldown: Some(2.0),
        },
        EnemyKind::LumenDrone => EnemyStats {
            health: 9.0,
            radius: 17.0,
            speed: 96.0,
            fire_cooldown: Some(1.0),
        },
    }
}

/// Activate boss rage on `e` (spec IV.7): a 1.5 s invulnerability window, a
/// permanent ×0.66 fire cooldown, and an immediate 16-bullet circular tantrum
/// from `pos`. Shared by the HP-threshold trigger (`boss_rage`) and the
/// pair-link trigger (`boss_pair_rage`).
fn activate_rage(
    commands: &mut Commands,
    fire: &mut MessageWriter<Fire>,
    e: Entity,
    pos: Vec2,
    fc: Option<Mut<FireCooldown>>,
) {
    commands
        .entity(e)
        .insert(Raged)
        .insert(Invulnerable { seconds: 1.5 });
    if let Some(mut fc) = fc {
        fc.cooldown *= 0.66;
        fc.timer = 0.0; // fire again immediately
    }
    for i in 0..16 {
        let a = i as f32 / 16.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        fire.write(Fire {
            origin: pos + dir * 24.0,
            dir,
            damage: 3.0,
            speed: 280.0,
            faction: Faction::Enemy,
            element: crate::combat::element::Element::Kinetic,
            // Raged bosses fire homing bullets (spec IV.7 `enableHomingBullets`).
            homing: true,
        });
    }
}

/// Gentle turn rate (rad/sec) for raged-boss homing bullets — the bounded
/// `steer_toward` equivalent of the JS per-tick `vel += dir*0.04` nudge
/// (spec IV.5): a ~0.04 px/tick nudge on a ~5 px/tick bullet ≈ 0.008 rad/tick
/// ≈ 0.48 rad/sec.
pub const RAGE_HOMING_TURN: f32 = 0.5;

/// Curve raged-boss bullets (`RageHoming`) toward the player, preserving speed
/// (spec IV.7 / IV.5). Distinct from `power_weapon::homing_steer`, which targets
/// the nearest enemy for *player* missiles.
pub fn rage_homing_steer(
    time: Res<Time>,
    player: Query<&Transform, With<Ship>>,
    mut bullets: Query<(&mut Velocity, &Transform, &RageHoming)>,
) {
    let Ok(player_tf) = player.single() else { return };
    let player_pos = player_tf.translation.truncate();
    let dt = time.delta_secs();
    for (mut vel, tf, homing) in &mut bullets {
        let to_player = (player_pos - tf.translation.truncate()).normalize_or_zero();
        if to_player == Vec2::ZERO {
            continue;
        }
        vel.0 = crate::systems::power_weapon::steer_toward(vel.0, to_player, homing.turn_rate * dt);
    }
}

/// Rage telegraph duration (`TELEGRAPH_FRAMES = 24` @60 Hz ≈ 0.4 s, spec IV.7).
pub const TELEGRAPH_SECS: f32 = 24.0 / 60.0;

/// A red HDR warning ring (lyon stroke) at `radius`, emissive so bloom makes it
/// glow — the rage telegraph aura (`radius*1.35`, spec IV.7).
fn telegraph_ring(radius: f32) -> Shape {
    let mut path = ShapePath::new();
    for i in 0..32 {
        let a = i as f32 / 32.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .stroke((Color::linear_rgb(9.0, 0.5, 0.6), 3.0))
        .build()
}

/// An element-colored halo ring (HDR-boosted from `Element::color` so bloom
/// flares it) worn by elemental enemies so they read at a glance — many reuse a
/// non-elemental silhouette (e.g. Cinder = Wasp), so the tint is the tell.
fn element_ring(radius: f32, element: Element) -> Shape {
    let l = element.color().to_linear();
    let glow = Color::linear_rgb(l.red * 3.0, l.green * 3.0, l.blue * 3.0);
    let mut path = ShapePath::new();
    for i in 0..28 {
        let a = i as f32 / 28.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).stroke((glow, 2.0)).build()
}

/// A small amber turret silhouette for a boss weak-point [`BossPart`].
fn boss_part_shape() -> Shape {
    let r = 12.0_f32;
    let mut path = ShapePath::new();
    for i in 0..6 {
        let a = i as f32 / 6.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(0.1, 0.05, 0.02))
        .stroke((Color::linear_rgb(8.0, 3.5, 0.4), 2.0)) // amber emissive edge
        .build()
}

/// Spawn a ring of `PART_COUNT` destructible weak-point [`BossPart`]s around a
/// boss (rainboids `boss-parts.js`): each is an inert `Enemy` body (no Velocity →
/// AI skips it; no FireCooldown → it doesn't fire), parked at its `offset` by
/// `mechanics::update_boss_parts`. While they live the boss core is invulnerable
/// (`mechanics::update_core_shield` + the gate in `collision::bullet_hits_enemy`).
pub fn spawn_boss_parts(commands: &mut Commands, boss: Entity, center: Vec2, ring: f32, hp: f32) {
    const PART_COUNT: usize = 3;
    for i in 0..PART_COUNT {
        let a = i as f32 / PART_COUNT as f32 * std::f32::consts::TAU;
        let offset = Vec2::new(a.cos(), a.sin()) * ring;
        commands.spawn((
            Enemy { kind: EnemyKind::Hunter }, // a neutral hittable body
            BossPart { boss, shields_core: true, offset },
            Health::new(hp),
            Collider { radius: 14.0 },
            Faction::Enemy,
            boss_part_shape(),
            Transform::from_translation((center + offset).extend(0.2)),
        ));
    }
}

/// Per-part HP for a tier-`tier` boss's weak points — tankier on higher tiers so
/// stripping a big boss's shield takes longer (`30 + 20 × tier`).
pub fn boss_part_hp(tier: u8) -> f32 {
    30.0 + 20.0 * tier as f32
}

/// HP-threshold boss rage (spec IV.7, one-shot): when a boss drops to ≤33% HP it
/// enters the **telegraph** window — a red warning ring + a `RageTelegraph` timer
/// — rather than raging instantly. `tick_rage_telegraph` fires `activate_rage`
/// when the timer lapses. (Deferred: screen flash/shake, ember particles,
/// tier-3+ formations.)
pub fn boss_rage(
    mut commands: Commands,
    bosses: Query<
        (Entity, &Transform, &Health, Option<&Collider>),
        (With<Boss>, Without<Raged>, Without<RageTelegraph>),
    >,
) {
    for (e, tf, hp, collider) in &bosses {
        if hp.current > hp.max * 0.33 {
            continue;
        }
        commands.entity(e).insert(RageTelegraph {
            timer: TELEGRAPH_SECS,
        });
        // Warning ring at radius*1.35 (spec IV.7 aura). Top-level entity so it
        // ignores the boss's Transform scale; self-despawns when the telegraph
        // lapses (`Lifetime == TELEGRAPH_SECS`). Bosses barely move in 0.4 s.
        let radius = collider.map_or(40.0, |c| c.radius) * 1.35;
        let pos = tf.translation.truncate();
        commands.spawn((
            TelegraphRing,
            telegraph_ring(radius),
            Transform::from_translation(pos.extend(0.3)),
            Lifetime {
                seconds: TELEGRAPH_SECS,
            },
        ));
    }
}

/// Boss fight phase from its HP fraction: 0 = calm (>66%), 1 = frenzied
/// (33–66%, `boss_frenzy`), 2 = raged (≤33%, `boss_rage`). Pure for testing.
pub fn boss_phase(hp_frac: f32) -> u8 {
    if hp_frac > 0.66 {
        0
    } else if hp_frac > 0.33 {
        1
    } else {
        2
    }
}

/// Mid-fight **frenzy** escalation (one-shot): when a boss first drops into the
/// 33–66% HP band it speeds up its fire and looses a ring burst — a milder beat
/// before the ≤33% rage (`boss_rage`), so the fight escalates in two phases.
/// Non-homing (the homing/invuln rage is the harder, later phase).
pub fn boss_frenzy(
    mut commands: Commands,
    mut fire: MessageWriter<Fire>,
    sfx: Option<Res<crate::audio::Sfx>>,
    player: Query<&Transform, With<Ship>>,
    mut bosses: Query<
        (Entity, &Transform, &Health, Option<&mut FireCooldown>),
        (With<Boss>, Without<Raged>, Without<Frenzied>),
    >,
) {
    // Frenzy aims at the player (a focused fan), distinct from the rage ring.
    let player_pos = player.single().ok().map(|t| t.translation.truncate());
    for (e, tf, hp, fc) in &mut bosses {
        if boss_phase(hp.current / hp.max) != 1 {
            continue; // only on first entry to the mid band
        }
        commands.entity(e).insert(Frenzied);
        if let Some(sfx) = &sfx {
            commands.spawn((
                AudioPlayer::new(sfx.boss_roar.clone()),
                PlaybackSettings::DESPAWN,
            ));
        }
        if let Some(mut fc) = fc {
            fc.cooldown *= 0.8; // a gentler speed-up than rage's ×0.66
            fc.timer = 0.0;
        }
        let pos = tf.translation.truncate();
        // A one-shot amber power-up ring so the escalation reads (the ≤33% rage
        // has its telegraph; this gives the 66% frenzy a matching visual beat).
        commands.spawn((
            crate::render::reaction_fx::Shockwave { age: 0.0, peak: 90.0 },
            crate::render::reaction_fx::unit_ring(Color::linear_rgb(8.0, 4.0, 0.5)),
            Transform::from_translation(pos.extend(0.3)).with_scale(Vec3::splat(1.0)),
        ));
        // A focused 5-shot fan aimed at the player (falls back to firing downward
        // if the player is absent) — reads as "the boss locks on", unlike the
        // omnidirectional rage ring.
        let aim = player_pos
            .map(|p| (p - pos).normalize_or_zero())
            .filter(|d| *d != Vec2::ZERO)
            .unwrap_or(Vec2::NEG_Y);
        let base = aim.to_angle();
        const FAN: usize = 5;
        const STEP: f32 = 0.18; // rad between fan shots (~10°)
        for i in 0..FAN {
            let off = (i as f32 - (FAN as f32 - 1.0) / 2.0) * STEP;
            let a = base + off;
            let dir = Vec2::new(a.cos(), a.sin());
            fire.write(Fire {
                origin: pos + dir * 24.0,
                dir,
                damage: 3.0,
                speed: 280.0,
                faction: Faction::Enemy,
                element: Element::Kinetic,
                homing: false,
            });
        }
    }
}

/// Tick the rage telegraph; when it lapses, fire the actual rage (spec IV.7).
/// Skips bosses already `Raged` (e.g. a pair-link rage that pre-empted the
/// telegraph) so the tantrum never double-fires.
pub fn tick_rage_telegraph(
    time: Res<Time>,
    mut commands: Commands,
    mut fire: MessageWriter<Fire>,
    sfx: Option<Res<crate::audio::Sfx>>,
    mut bosses: Query<
        (Entity, &Transform, &mut RageTelegraph, Option<&mut FireCooldown>),
        Without<Raged>,
    >,
) {
    let dt = time.delta_secs();
    for (e, tf, mut tel, fc) in &mut bosses {
        tel.timer -= dt;
        if tel.timer > 0.0 {
            continue;
        }
        commands.entity(e).remove::<RageTelegraph>();
        activate_rage(&mut commands, &mut fire, e, tf.translation.truncate(), fc);
        if let Some(sfx) = &sfx {
            commands.spawn((
                AudioPlayer::new(sfx.boss_roar.clone()),
                PlaybackSettings::DESPAWN,
            ));
        }
    }
}

/// Boss-pair rage link (spec IV.7, tier 2): when *any* boss dies, every
/// surviving un-raged boss immediately rages.
pub fn boss_pair_rage(
    mut deaths: MessageReader<Death>,
    mut commands: Commands,
    mut fire: MessageWriter<Fire>,
    mut bosses: Query<(Entity, &Transform, Option<&mut FireCooldown>), (With<Boss>, Without<Raged>)>,
) {
    if !deaths.read().any(|d| d.boss_tier > 0) {
        return;
    }
    for (e, tf, fc) in &mut bosses {
        activate_rage(&mut commands, &mut fire, e, tf.translation.truncate(), fc);
    }
}

/// Roster point value for destroying this kind (spec IV.1 "Points" column).
pub fn points(kind: EnemyKind) -> u64 {
    match kind {
        EnemyKind::Hunter => 120,
        EnemyKind::Guardian => 200,
        EnemyKind::Wasp => 100,
        EnemyKind::Stalker => 130,
        EnemyKind::Drifter => 180,
        EnemyKind::Prowler => 240,
        EnemyKind::Weaver => 160,
        EnemyKind::Sentinel => 220,
        EnemyKind::Tangerine => 160,
        EnemyKind::Titan => 320,
        // EN batch E8b points (enemy-data.js:392-465).
        EnemyKind::Cinder => 110,
        EnemyKind::Glacier => 250,
        EnemyKind::FrostLance => 150,
        EnemyKind::AshenDetonator => 160,
        // EN batch E8c points (enemy-data.js:473-577).
        EnemyKind::TeslaWraith => 150,
        EnemyKind::Plaguebearer => 200,
        EnemyKind::SporeCarrier => 240,
        EnemyKind::Hydra => 220,
        // EN batch E8d/e points.
        EnemyKind::Warden => 280,
        EnemyKind::LumenDrone => 220,
    }
}

/// Per-tier boss point value (spec IV.7 — overrides the base roster points).
pub fn boss_points(tier: u8) -> u64 {
    match tier {
        1 => 500,
        2 => 1000,
        3 => 1750,
        4 => 3000,
        _ => 0,
    }
}

/// `ENEMY_DROP_PROFILES` gold-budget multiplier (spec VI.5). A boss overrides
/// the per-kind profile with the 2.4× boss budget.
pub fn drop_budget_mul(kind: EnemyKind, boss: bool) -> f32 {
    if boss {
        return 2.4; // boss profile
    }
    match kind {
        // grunt
        EnemyKind::Hunter | EnemyKind::Wasp | EnemyKind::Stalker => 0.75,
        // standard
        EnemyKind::Drifter | EnemyKind::Weaver | EnemyKind::Tangerine => 1.0,
        // tanky
        EnemyKind::Guardian | EnemyKind::Prowler | EnemyKind::Sentinel => 1.4,
        // miniboss
        EnemyKind::Titan => 1.8,
        // EN batch E8b drop profiles: Cinder grunt, Glacier tanky, the rest standard.
        EnemyKind::Cinder => 0.75,
        EnemyKind::Glacier => 1.4,
        EnemyKind::FrostLance | EnemyKind::AshenDetonator => 1.0,
        // EN batch E8c: Tesla grunt, Plague/Spore/Hydra tanky standoffs.
        EnemyKind::TeslaWraith => 0.75,
        EnemyKind::Plaguebearer | EnemyKind::SporeCarrier | EnemyKind::Hydra => 1.4,
        // EN batch E8d/e: Warden tanky, Lumen standard.
        EnemyKind::Warden => 1.4,
        EnemyKind::LumenDrone => 1.0,
    }
}

/// Boss-tier stat multipliers `(hp, size, speed)` from `BOSS_TIER_STATS`
/// (port spec IV.7). Tier 0 = a normal (non-boss) enemy.
pub fn boss_tier_mul(tier: u8) -> (f32, f32, f32) {
    match tier {
        1 => (4.0, 1.35, 1.00),
        2 => (5.0, 1.45, 1.05),
        3 => (6.0, 1.55, 1.10),
        4 => (8.0, 1.75, 1.15),
        _ => (1.0, 1.0, 1.0),
    }
}

/// Mini-boss HP / size overlay (spec V.6: HP×1.7, radius×1.25).
const MINI_BOSS_HP_MUL: f32 = 1.7;
const MINI_BOSS_SZ_MUL: f32 = 1.25;

/// Mid-wave mini-boss promotion chance for a group at `wave`
/// (spec V.6: `min(0.45, 0.06 + (wave−4)*0.025)`, 0 below wave 4).
pub fn mini_boss_chance(wave: u64) -> f32 {
    if wave < 4 {
        0.0
    } else {
        (0.06 + (wave - 4) as f32 * 0.025).min(0.45)
    }
}

/// Campaign difficulty progress for a wave: `t = (w−1)/29`, clamped to `[0,1]`.
fn difficulty_t(wave: u64) -> f32 {
    (((wave.max(1) - 1) as f32) / 29.0).clamp(0.0, 1.0)
}

/// Campaign-wide enemy HP multiplier by wave (spec V.4): `1 + t*8 + t^2.5*6.5`
/// (W1 1.0× → W30 15.5×).
pub fn difficulty_hp_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    1.0 + t * 8.0 + t.powf(2.5) * 6.5
}

/// Campaign-wide enemy point multiplier by wave (spec V.4): `1 + t^1.4*5.5`
/// (W1 1.0× → W30 6.5×).
pub fn difficulty_points_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    1.0 + t.powf(1.4) * 5.5
}

/// Campaign-wide enemy *movement*-speed multiplier by wave (spec V.4
/// `0.55 + t^1.5*1.2`), **normalized to W1 = 1.0** (the port's per-kind speeds
/// are the W1-effective values) → W30 ≈ 3.18×.
pub fn difficulty_speed_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    (0.55 + t.powf(1.5) * 1.2) / 0.55
}

/// Campaign-wide enemy *bullet*-speed multiplier by wave (spec V.4 campaignMul
/// `1.15 + t^1.4*1.9`), **normalized to W1 = 1.0** — the port's per-kind bullet
/// speeds are already the W1-effective values, so only the relative ramp
/// applies (W1 1.0× → W30 ≈2.65×).
pub fn difficulty_bullet_speed_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    (1.15 + t.powf(1.4) * 1.9) / 1.15
}

/// What promotion (if any) an enemy spawns with.
#[derive(Clone, Copy)]
enum Promo {
    None,
    Boss(u8),
    Mini,
}

/// Spawn one enemy of `kind` at world position `pos` (non-boss).
pub fn spawn(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    spawn_tiered(commands, kind, pos, 0);
}

/// Spawn one enemy, applying a boss-tier HP/size overlay when `tier > 0`.
/// (Speed-scaling is a follow-up — AI reads a fixed per-kind `stats().speed`,
/// so it needs an entity-stored multiplier, tracked separately.)
pub fn spawn_tiered(commands: &mut Commands, kind: EnemyKind, pos: Vec2, tier: u8) {
    let (hp_mul, sz_mul, sp_mul) = boss_tier_mul(tier);
    let promo = if tier > 0 { Promo::Boss(tier) } else { Promo::None };
    let boss = spawn_enemy(commands, kind, pos, hp_mul, sz_mul, sp_mul, promo);
    // A tier boss carries a ring of destructible weak-point parts that shield its
    // core (BO) — clear them to expose the boss. Ring scales with the boss size.
    if tier > 0 {
        spawn_boss_parts(commands, boss, pos, 46.0 * sz_mul, boss_part_hp(tier));
    }
}

/// Spawn a mid-wave **mini-boss** promotion of `kind` (spec V.6 — no speed buff).
pub fn spawn_mini_boss(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    spawn_enemy(
        commands,
        kind,
        pos,
        MINI_BOSS_HP_MUL,
        MINI_BOSS_SZ_MUL,
        1.0,
        Promo::Mini,
    );
}

/// Spawn a Spore Carrier **drone** — a Wasp tagged [`Drone`] (so it counts
/// against the spawner cap) at `pos`.
pub fn spawn_drone(commands: &mut Commands, pos: Vec2) {
    let e = spawn_enemy(commands, EnemyKind::Wasp, pos, 1.0, 1.0, 1.0, Promo::None);
    commands.entity(e).insert(Drone);
}

/// Spawn a Hydra **ling** (EN split-on-death): a half-size (×0.5 HP / ×0.7 size)
/// copy at `pos` that can't split again (`Splitter { lings: 0 }` overrides the
/// `lings: 2` that `spawn_enemy` stamps on a Hydra).
pub fn spawn_split_ling(commands: &mut Commands, pos: Vec2) {
    let e = spawn_enemy(commands, EnemyKind::Hydra, pos, 0.5, 0.7, 1.0, Promo::None);
    commands.entity(e).insert(Splitter { lings: 0 });
}

/// Wave-aware spawn used by the campaign: applies the V.4 HP difficulty curve on
/// top of the tier / mini-boss HP overlay. `mini` promotes to a mini-boss
/// (ignoring `tier`); otherwise `tier` selects the boss overlay (0 = normal).
pub fn spawn_for_wave(
    commands: &mut Commands,
    kind: EnemyKind,
    pos: Vec2,
    tier: u8,
    mini: bool,
    wave: u64,
) -> Entity {
    let diff_hp = difficulty_hp_mul(wave);
    let diff_sp = difficulty_speed_mul(wave);
    if mini {
        spawn_enemy(
            commands,
            kind,
            pos,
            MINI_BOSS_HP_MUL * diff_hp,
            MINI_BOSS_SZ_MUL,
            diff_sp,
            Promo::Mini,
        )
    } else {
        let (hp_mul, sz_mul, sp_mul) = boss_tier_mul(tier);
        let promo = if tier > 0 { Promo::Boss(tier) } else { Promo::None };
        spawn_enemy(commands, kind, pos, hp_mul * diff_hp, sz_mul, sp_mul * diff_sp, promo)
    }
}

/// Core spawn used by all variants: build the enemy with HP/size multipliers and
/// the given promotion marker. Returns the spawned `Entity` so callers (e.g. the
/// wave pulse) can bundle a group into a formation.
fn spawn_enemy(
    commands: &mut Commands,
    kind: EnemyKind,
    pos: Vec2,
    hp_mul: f32,
    sz_mul: f32,
    speed_mul: f32,
    promo: Promo,
) -> Entity {
    let st = stats_for(kind);

    let mut e = commands.spawn((
        Enemy { kind },
        AiState::default(),
        Velocity::default(),
        Collider { radius: st.radius * sz_mul },
        Health::new(st.health * hp_mul),
        SpeedMul(speed_mul),
        Faction::Enemy,
        shape_for(kind),
        // Start small + carry a WarpIn marker so `render::warp_in` materializes
        // the silhouette up to `sz_mul`. The Collider above is already full-size,
        // so the warp is purely cosmetic (engagement timing is unchanged).
        WarpIn { elapsed: 0.0, dur: 0.35, scale: sz_mul },
        Transform::from_translation(pos.extend(0.0)).with_scale(Vec3::splat(sz_mul * 0.3)),
    ));
    // Elemental resistance map (E2) — read by the player→enemy damage path.
    e.insert(resistances_for(kind));
    // Flat armor (Guardian) + frontal shield (Sentinel) archetypes (E4,
    // enemy-data.js ENEMY_ARMOR / ENEMY_FRONTAL_SHIELD).
    match kind {
        EnemyKind::Guardian => {
            e.insert(Armor(1.0));
        }
        EnemyKind::Sentinel => {
            e.insert(FrontalShield { arc: 2.4, reduction: 0.8 });
        }
        // Hydra splits into 2 lings on death (EN). Lings are re-stamped with
        // `lings: 0` by `spawn_split_ling` so they don't split again.
        EnemyKind::Hydra => {
            e.insert(Splitter { lings: 2 });
        }
        // Spore Carrier births Wasp drones on a timer (EN).
        EnemyKind::SporeCarrier => {
            e.insert(DroneSpawner {
                timer: SPORE_DRONE_INTERVAL,
            });
        }
        // Plaguebearer drips a Toxic acid trail as it moves (EN).
        EnemyKind::Plaguebearer => {
            e.insert(crate::systems::hazard::plaguebearer_dropper());
        }
        // Lumen Drone shields nearby allies on a timer (EN).
        EnemyKind::LumenDrone => {
            e.insert(SupportAura {
                radius: AURA_RADIUS,
                amount: AURA_AMOUNT,
                interval: AURA_INTERVAL,
                timer: AURA_INTERVAL,
            });
        }
        // Warden learns to resist the element you spam at it (EN).
        EnemyKind::Warden => {
            e.insert(Adaptive);
        }
        _ => {}
    }

    match promo {
        Promo::Boss(tier) => {
            e.insert(Boss { tier });
        }
        Promo::Mini => {
            e.insert(MiniBoss);
        }
        Promo::None => {}
    }

    if let Some(cd) = st.fire_cooldown {
        e.insert(FireCooldown {
            cooldown: cd,
            timer: cd * 0.5,
        });
    }

    // The Drifter keeps its original drift velocity + white-hot core child.
    if kind == EnemyKind::Drifter {
        e.insert(Velocity(Vec2::new(55.0, -18.0)));
        e.with_children(|c| {
            c.spawn((
                shapes::drifter_core(18.0),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ));
        });
    }

    // Elemental enemies wear an element-colored halo so they read at a glance.
    let elem = element_for(kind);
    if elem != Element::Kinetic {
        e.with_children(|c| {
            c.spawn((
                element_ring(st.radius * 1.2, elem),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
        });
    }

    e.id()
}
