//! Tower-defense systems: the **Core** (the central objective the player
//! defends) and — added in later slices — the player-built turrets, their
//! auto-targeting fire, and the placement economy.
//!
//! Conversion note: this game began life as a bullet-hell shooter where the
//! *player ship* was the lose condition. In tower-defense the ship is a mobile
//! commander that never ends the run; only the Core dying does. Enemy AIs
//! navigate toward the Core (`With<Core>` replaced `With<Ship>` in their seek
//! queries); they ram it for a "leak" (`collision::enemy_contact_core`), and
//! `core_lose_check` flips to `GameOver` when its `Health` is spent.

use crate::combat::element::{Element, ElementSet};
use crate::components::{
    Airburst, Bullet, BulletElements, BulletKind, Collider, Core, Enemy, Faction, GravityBullet,
    Health, Lifetime, Overdrive, Velocity, OVERDRIVE_FIRE_MULT,
};
use crate::render::bullets::BulletAssets;
use crate::resources::{Aim, PlayBounds, Score};
use crate::states::GameState;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── The Core (defense objective) ─────────────────────────────────────────────

/// Starting Core integrity. The run is lost when this is spent. Tuned against
/// the per-enemy leak (`collision::CORE_LEAK_*`) so a few unchecked breaches in
/// the early waves sting without instantly ending the run.
pub const CORE_MAX_HP: f32 = 1000.0;

/// Core collision radius (a large central hub). Used by `enemy_contact_core`
/// for the ram check and by placement validation to keep its footprint clear.
pub const CORE_RADIUS: f32 = 54.0;

/// Spawn the Core at world origin on entering a run (`OnTransition Title→Playing`,
/// alongside `spawn::spawn_player`). It carries `Health` (the integrity bar),
/// a `Collider` for the leak check, and `Faction::Player` so it reads as
/// friendly. It is *not* an `Enemy` and *not* a `Ship`, so the bullet/contact/
/// movement/cleanup systems leave it untouched.
pub fn spawn_core(mut commands: Commands) {
    commands
        .spawn((
            Core,
            Health::new(CORE_MAX_HP),
            Collider { radius: CORE_RADIUS },
            Faction::Player,
            core_shape(),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .with_children(|core| {
            // Inner pulsing heart — a brighter disc behind the ring.
            core.spawn((
                core_heart_shape(),
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
            ));
        });
}

/// Lose condition: when the Core's `Health` is spent, end the run. Runs while
/// `Playing`; the existing `GameOver` screen flow handles the rest.
pub fn core_lose_check(
    core: Query<&Health, With<Core>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if let Ok(hp) = core.single() {
        if hp.current <= 0.0 {
            next.set(GameState::GameOver);
        }
    }
}

// ── Shapes ───────────────────────────────────────────────────────────────────

/// The Core silhouette: an emissive cyan octagonal ring (HDR stroke → bloom),
/// dark interior. Authored directly in Bevy space (+Y up).
fn core_shape() -> Shape {
    let r = CORE_RADIUS;
    let mut path = ShapePath::new();
    for i in 0..8 {
        // Offset by π/8 so a flat edge faces up (a hub, not a diamond).
        let a = i as f32 / 8.0 * std::f32::consts::TAU + std::f32::consts::FRAC_PI_8;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    path = path.close();
    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.0, 0.05, 0.08))
        // HDR cyan edge — the station's shield ring, boosted for bloom.
        .stroke((Color::linear_rgb(0.6, 6.0, 9.0), 3.0))
        .build()
}

/// The Core's inner heart: a small bright cyan disc that reads as a reactor.
fn core_heart_shape() -> Shape {
    let r = CORE_RADIUS * 0.45;
    let mut path = ShapePath::new();
    for i in 0..24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    path = path.close();
    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.4, 4.5, 7.0))
        .build()
}

// ── Towers ───────────────────────────────────────────────────────────────────

/// The v1 turret roster. Each kind reuses an existing projectile archetype +
/// carries an element, so the shared bullet → collision → damage → reaction
/// pipeline does all the work (e.g. a Frost turret freezes, an Inferno turret's
/// heavy hit then shatters the frozen target).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TowerKind {
    /// Cheap, fast single shots (Kinetic).
    Gun,
    /// Airbursts into a shrapnel ring at range — anti-swarm AoE (Kinetic).
    Flak,
    /// Cryo — freezes the target (hard CC; sets up SHATTER).
    Frost,
    /// Pyro — burning DoT, and its heavy hit shatters frozen targets.
    Inferno,
    /// Void gravity orb — drags nearby enemies together (soft CC).
    Gravity,
}

impl TowerKind {
    /// The roster in selection order (number keys 1‑5).
    pub const ALL: [TowerKind; 5] = [
        TowerKind::Gun,
        TowerKind::Flak,
        TowerKind::Frost,
        TowerKind::Inferno,
        TowerKind::Gravity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TowerKind::Gun => "Gun",
            TowerKind::Flak => "Flak",
            TowerKind::Frost => "Frost",
            TowerKind::Inferno => "Inferno",
            TowerKind::Gravity => "Gravity",
        }
    }

    /// Build cost in gold (`Score.gold`).
    pub fn cost(self) -> u64 {
        match self {
            TowerKind::Gun => 40,
            TowerKind::Frost => 70,
            TowerKind::Flak => 80,
            TowerKind::Inferno => 90,
            TowerKind::Gravity => 100,
        }
    }

    /// The element each shot carries — drives resist, statuses, and reactions.
    pub fn element(self) -> Element {
        match self {
            TowerKind::Gun | TowerKind::Flak => Element::Kinetic,
            TowerKind::Frost => Element::Cryo,
            TowerKind::Inferno => Element::Pyro,
            TowerKind::Gravity => Element::Void,
        }
    }

    /// HDR tint for the turret silhouette (its element identity colour, boosted).
    fn color(self) -> Color {
        match self {
            TowerKind::Gun => Color::linear_rgb(7.0, 7.0, 2.0),
            TowerKind::Flak => Color::linear_rgb(7.0, 4.0, 1.0),
            TowerKind::Frost => Color::linear_rgb(1.0, 5.0, 9.0),
            TowerKind::Inferno => Color::linear_rgb(9.0, 2.0, 0.5),
            TowerKind::Gravity => Color::linear_rgb(4.0, 2.0, 8.0),
        }
    }

    /// Combat stats at level 0 (placement). `level_*_mult` scales them on upgrade.
    fn stats(self) -> TowerStats {
        match self {
            TowerKind::Gun => TowerStats { range: 260.0, cooldown: 0.5, damage: 8.0, bullet_speed: 700.0, bullet_radius: 5.0, bullet_life: 1.0, pierce: 0 },
            TowerKind::Flak => TowerStats { range: 220.0, cooldown: 1.4, damage: 6.0, bullet_speed: 480.0, bullet_radius: 6.0, bullet_life: 1.2, pierce: 0 },
            TowerKind::Frost => TowerStats { range: 240.0, cooldown: 1.0, damage: 4.0, bullet_speed: 620.0, bullet_radius: 5.0, bullet_life: 1.0, pierce: 0 },
            TowerKind::Inferno => TowerStats { range: 230.0, cooldown: 0.9, damage: 7.0, bullet_speed: 600.0, bullet_radius: 6.0, bullet_life: 1.0, pierce: 0 },
            TowerKind::Gravity => TowerStats { range: 280.0, cooldown: 1.6, damage: 5.0, bullet_speed: 360.0, bullet_radius: 7.0, bullet_life: 1.4, pierce: 1 },
        }
    }
}

/// Level-0 combat profile for a [`TowerKind`].
struct TowerStats {
    range: f32,
    cooldown: f32,
    damage: f32,
    bullet_speed: f32,
    bullet_radius: f32,
    bullet_life: f32,
    pierce: u32,
}

/// A placed turret. Auto-targets the nearest enemy in `range` and fires its
/// kind's projectile on a cooldown. `level` (0-based) scales damage + range.
#[derive(Component, Debug)]
pub struct Tower {
    pub kind: TowerKind,
    pub range: f32,
    pub cooldown: f32,
    /// Counts down to 0; fires (and resets to `cooldown`) when ready and a target
    /// is in range.
    pub timer: f32,
    pub level: u8,
    /// Total gold sunk into this tower (placement + upgrades) — sell refunds a
    /// fraction of it.
    pub spent: u64,
}

impl Tower {
    /// A fresh level-0 tower of `kind`, ready to fire immediately.
    pub fn new(kind: TowerKind) -> Self {
        let s = kind.stats();
        Self {
            kind,
            range: s.range,
            cooldown: s.cooldown,
            timer: 0.0,
            level: 0,
            spent: kind.cost(),
        }
    }
}

/// Per-level damage multiplier (+30% per upgrade level).
pub fn level_damage_mult(level: u8) -> f32 {
    1.0 + 0.30 * level as f32
}
/// Per-level range multiplier (+8% per upgrade level).
pub fn level_range_mult(level: u8) -> f32 {
    1.0 + 0.08 * level as f32
}

/// Distance a tower Flak round travels before it airbursts.
const TOWER_FLAK_BURST_DIST: f32 = 200.0;

/// The nearest candidate position within `range` of `origin`, or `None`. Pure
/// (the turret target-acquisition rule) so it is unit-testable.
pub fn nearest_in_range(origin: Vec2, range: f32, candidates: &[Vec2]) -> Option<Vec2> {
    let r2 = range * range;
    candidates
        .iter()
        .copied()
        .filter(|p| p.distance_squared(origin) <= r2)
        .min_by(|a, b| {
            a.distance_squared(origin)
                .partial_cmp(&b.distance_squared(origin))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Auto-targeting turret fire (FixedUpdate). Each ready tower locks the nearest
/// enemy within range and spawns its projectile — `Faction::Player` /
/// `BulletKind::Player`, so `collision::bullet_hits_enemy` + the damage/reaction
/// pipeline handle it exactly like the commander's fire.
pub fn tower_fire(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<BulletAssets>,
    mut towers: Query<(&Transform, &mut Tower)>,
    enemies: Query<&Transform, With<Enemy>>,
    core: Query<Has<Overdrive>, With<Core>>,
) {
    let dt = time.delta_secs();
    // Overdrive (power weapon) speeds up turret fire while active on the Core.
    let fire_mult = if core.single().unwrap_or(false) {
        OVERDRIVE_FIRE_MULT
    } else {
        1.0
    };
    // Snapshot enemy positions once per tick (shared across all towers).
    let positions: Vec<Vec2> = enemies.iter().map(|t| t.translation.truncate()).collect();
    for (ttf, mut tower) in &mut towers {
        tower.timer -= dt;
        if tower.timer > 0.0 {
            continue;
        }
        let tpos = ttf.translation.truncate();
        let Some(tgt) = nearest_in_range(tpos, tower.range, &positions) else {
            tower.timer = 0.0; // primed — fire the instant something enters range
            continue;
        };
        let dir = (tgt - tpos).normalize_or_zero();
        if dir == Vec2::ZERO {
            continue;
        }
        tower.timer = tower.cooldown * fire_mult;
        spawn_tower_bullet(&mut commands, &assets, tower.kind, tower.level, tpos, dir);
    }
}

/// Spawn one turret projectile. Mirrors the player-bullet bundle in
/// `weapons::spawn_bullets` (reusing `BulletAssets`), tagged with the tower's
/// element + any archetype component (airburst / gravity).
fn spawn_tower_bullet(
    commands: &mut Commands,
    assets: &BulletAssets,
    kind: TowerKind,
    level: u8,
    origin: Vec2,
    dir: Vec2,
) {
    let s = kind.stats();
    let damage = s.damage * level_damage_mult(level);
    let speed = s.bullet_speed;
    let radius = s.bullet_radius;
    let mut e = commands.spawn((
        Bullet { kind: BulletKind::Player, damage, pierce: s.pierce },
        Velocity(dir * speed),
        Collider { radius },
        Faction::Player,
        Lifetime { seconds: s.bullet_life },
        Mesh2d(assets.circle.clone()),
        MeshMaterial2d(assets.player_body.clone()),
        Transform::from_translation(origin.extend(0.0)).with_scale(Vec3::splat(radius)),
        BulletElements(ElementSet::single(kind.element())),
    ));
    match kind {
        // Flak rounds airburst into a shrapnel ring after a short flight.
        TowerKind::Flak => {
            e.insert(Airburst { timer: TOWER_FLAK_BURST_DIST / speed.max(1.0) });
        }
        // Gravity orbs drag nearby enemies inward as they fly.
        TowerKind::Gravity => {
            e.insert(GravityBullet { pull_radius: 150.0, pull_strength: 60.0 });
        }
        _ => {}
    }
}

/// Turret silhouette: a hexagonal emissive housing tinted to the kind's element.
/// Used by placement to give each built tower its body.
pub fn tower_shape(kind: TowerKind) -> Shape {
    let r = 16.0;
    let mut path = ShapePath::new();
    for i in 0..6 {
        let a = i as f32 / 6.0 * std::f32::consts::TAU + std::f32::consts::FRAC_PI_6;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    path = path.close();
    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.02, 0.03, 0.05))
        .stroke((kind.color(), 2.5))
        .build()
}

// ── Placement & economy ──────────────────────────────────────────────────────

/// Minimum centre-to-centre gap a new tower must keep from the Core (don't smother it).
pub const MIN_CORE_GAP: f32 = CORE_RADIUS + 28.0;
/// Minimum spacing between two towers.
pub const MIN_TOWER_SPACING: f32 = 38.0;
/// Keep placements this far inside the play bounds.
pub const PLACE_EDGE_MARGIN: f32 = 24.0;
/// Cursor pick radius for selecting an existing tower to upgrade / sell.
pub const TOWER_PICK_RADIUS: f32 = 28.0;
/// Fraction of a tower's invested gold refunded on sell.
pub const SELL_REFUND_FRAC: f32 = 0.5;
/// Max upgrade level (0 = freshly placed).
pub const TOWER_MAX_LEVEL: u8 = 3;

/// Gold to upgrade a tower from its current `level` to the next.
pub fn upgrade_cost(kind: TowerKind, level: u8) -> u64 {
    (kind.cost() as f32 * 0.8 * (level + 1) as f32).round() as u64
}

/// Is `pos` a legal tower spot? Pure (no ECS) so it is unit-testable: inside the
/// play bounds (minus a margin), clear of the Core, and not stacked on another tower.
pub fn can_place(pos: Vec2, core: Vec2, towers: &[Vec2], bounds_half: Vec2) -> bool {
    if pos.x.abs() > bounds_half.x - PLACE_EDGE_MARGIN
        || pos.y.abs() > bounds_half.y - PLACE_EDGE_MARGIN
    {
        return false;
    }
    if pos.distance(core) < MIN_CORE_GAP {
        return false;
    }
    towers.iter().all(|t| pos.distance(*t) >= MIN_TOWER_SPACING)
}

/// The currently-armed build selection. `kind = None` ⇒ not placing.
#[derive(Resource, Default)]
pub struct SelectedTower {
    pub kind: Option<TowerKind>,
}

/// The translucent placement preview (a range ring drawn at the cursor).
#[derive(Component)]
pub struct TowerGhost {
    /// Last-rendered (kind, valid) so the ring is only re-tessellated on change.
    rendered: Option<(TowerKind, bool)>,
}

/// Clear the armed selection at the start of a fresh run.
pub fn reset_selection(mut sel: ResMut<SelectedTower>) {
    sel.kind = None;
}

/// Spawn the (hidden) placement-preview entity once at boot.
pub fn setup_tower_ghost(mut commands: Commands) {
    commands.spawn((
        TowerGhost { rendered: None },
        range_ring(1.0, Color::linear_rgb(0.2, 5.0, 0.8)),
        Transform::from_xyz(0.0, 0.0, 0.45),
        Visibility::Hidden,
    ));
}

/// A thin emissive circle of the given radius — the tower range preview ring.
fn range_ring(radius: f32, color: Color) -> Shape {
    let n = 48;
    let mut path = ShapePath::new();
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    path = path.close();
    ShapeBuilder::with(&path).stroke((color, 1.5)).build()
}

/// Nearest tower whose body is within `TOWER_PICK_RADIUS` of `cur` (for upgrade/sell).
fn pick_tower(towers: &Query<(Entity, &Transform, &mut Tower)>, cur: Vec2) -> Option<Entity> {
    towers
        .iter()
        .filter(|(_, t, _)| t.translation.truncate().distance(cur) <= TOWER_PICK_RADIUS)
        .min_by(|(_, a, _), (_, b, _)| {
            a.translation
                .truncate()
                .distance_squared(cur)
                .partial_cmp(&b.translation.truncate().distance_squared(cur))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(e, _, _)| e)
}

/// Build input (Playing): number keys 1‑5 arm a tower kind (re-press / `0` /
/// right-click disarms); left-click builds at the cursor (gold-gated + validated);
/// `U` upgrades / `X` sells the tower under the cursor. While armed, the
/// commander's fire is suppressed so a click builds instead of shooting.
pub fn tower_build_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sel: ResMut<SelectedTower>,
    mut score: ResMut<Score>,
    bounds: Res<PlayBounds>,
    aim: Res<Aim>,
    core: Query<&Transform, With<Core>>,
    mut towers: Query<(Entity, &Transform, &mut Tower)>,
) {
    // ── Arm / disarm ──
    const PICKS: [(KeyCode, usize); 5] = [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
    ];
    for (key, idx) in PICKS {
        if keys.just_pressed(key) {
            let k = TowerKind::ALL[idx];
            sel.kind = if sel.kind == Some(k) { None } else { Some(k) };
        }
    }
    if keys.just_pressed(KeyCode::Digit0) || mouse.just_pressed(MouseButton::Right) {
        sel.kind = None;
    }

    // World-space cursor (from the global Aim), if the cursor is in-window.
    let cursor = aim.active.then_some(aim.world);

    // ── Upgrade / sell the tower under the cursor ──
    if let Some(cur) = cursor {
        if keys.just_pressed(KeyCode::KeyU) {
            if let Some(e) = pick_tower(&towers, cur) {
                if let Ok((_, _, mut t)) = towers.get_mut(e) {
                    if t.level < TOWER_MAX_LEVEL {
                        let cost = upgrade_cost(t.kind, t.level);
                        if score.gold >= cost {
                            score.gold -= cost;
                            t.level += 1;
                            t.range = t.kind.stats().range * level_range_mult(t.level);
                            t.spent += cost;
                        }
                    }
                }
            }
        }
        if keys.just_pressed(KeyCode::KeyX) {
            if let Some(e) = pick_tower(&towers, cur) {
                if let Ok((_, _, t)) = towers.get(e) {
                    score.gold += (t.spent as f32 * SELL_REFUND_FRAC) as u64;
                }
                commands.entity(e).despawn();
            }
        }
    }

    // ── Place (left-click while armed) ──
    if let (Some(kind), Some(cur)) = (sel.kind, cursor) {
        if mouse.just_pressed(MouseButton::Left) {
            let core_pos = core
                .single()
                .map(|t| t.translation.truncate())
                .unwrap_or(Vec2::ZERO);
            let positions: Vec<Vec2> =
                towers.iter().map(|(_, t, _)| t.translation.truncate()).collect();
            if score.gold >= kind.cost() && can_place(cur, core_pos, &positions, bounds.half) {
                score.gold -= kind.cost();
                commands.spawn((
                    Tower::new(kind),
                    tower_shape(kind),
                    Transform::from_translation(cur.extend(0.4)),
                ));
            }
        }
    }
}

/// Drive the placement preview ring: follow the cursor while armed in `Playing`,
/// tinted green (legal) / red (blocked); hidden otherwise. The ring is only
/// re-tessellated when the kind or validity changes.
pub fn update_tower_ghost(
    mut commands: Commands,
    state: Res<State<GameState>>,
    sel: Res<SelectedTower>,
    bounds: Res<PlayBounds>,
    aim: Res<Aim>,
    core: Query<&Transform, With<Core>>,
    towers: Query<&Transform, With<Tower>>,
    mut ghost: Query<
        (Entity, &mut Transform, &mut Visibility, &mut TowerGhost),
        (With<TowerGhost>, Without<Core>, Without<Tower>),
    >,
) {
    let Ok((ge, mut gtf, mut gvis, mut gstate)) = ghost.single_mut() else {
        return;
    };
    let armed = matches!(state.get(), GameState::Playing);
    let cursor = aim.active.then_some(aim.world);
    let (Some(kind), Some(cur)) = (sel.kind.filter(|_| armed), cursor) else {
        *gvis = Visibility::Hidden;
        return;
    };
    *gvis = Visibility::Visible;
    gtf.translation = cur.extend(0.45);
    let core_pos = core
        .single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);
    let positions: Vec<Vec2> = towers.iter().map(|t| t.translation.truncate()).collect();
    let valid = can_place(cur, core_pos, &positions, bounds.half);
    if gstate.rendered != Some((kind, valid)) {
        gstate.rendered = Some((kind, valid));
        let color = if valid {
            Color::linear_rgb(0.2, 5.0, 0.8)
        } else {
            Color::linear_rgb(6.0, 0.4, 0.4)
        };
        commands
            .entity(ge)
            .insert(range_ring(kind.stats().range, color));
    }
}

