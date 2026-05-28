//! On-demand upgrade shop (spec VIII.4).
//!
//! Opened with **B** during play (→ `GameState::Shop`, which pauses the sim);
//! closed with **B/Esc**. Single currency = gold. Cost of the next stack is the
//! exact spec formula `base × UPGRADE_COST_MULT(13) × 1.6^(owned)`, rounded to
//! 500 with a 500 floor (`upgrade_cost`).
//!
//! Implemented upgrades = the player-stat (DEFENSE) set, whose effects apply
//! directly to the ship entity on purchase. The weapon-effect upgrade trees
//! (charge/mine/nova/missile/lance/arc — spec VIII.4) need their per-weapon
//! mechanics first, so they're deferred behind this same framework + formula.

use crate::components::{Health, Lives, Ship, Shield, SHIELD_REDUCTION_CAP};
use crate::resources::Score;
use crate::states::GameState;
use bevy::prelude::*;

// ── Upgrade catalogue ──────────────────────────────────────────────────────

/// Shop upgrades: the player-stat (DEFENSE) set, applied to the ship on buy, plus
/// four primary-weapon traits (spec III.2) whose effect is read *live* by the
/// weapon systems from the `Upgrades` resource each shot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpgradeId {
    HealthBoost,
    ShieldBoost,
    SpeedBoost,
    SpareShip,
    Multishot,
    RapidFire,
    Piercing,
    BigShot,
    StunShot,
    HomingShot,
    ExplodeShot,
    KnockShot,
    Vampirism,
    Dodge,
    CritChance,
    CritDamage,
    Thorns,
    Regen,
    LastStand,
    Executioner,
    PhaseEcho,
    Overcharge,
    StaticDischarge,
    CombatMedic,
    Momentum,
    Whirlwind,
    /// Amplifies elemental-reaction (shatter / oil-flare) damage.
    Catalyst,
    /// Widens the elemental-reaction (shatter / oil-flare) blast radius.
    Detonator,
    /// Flat +% to all weapon damage.
    Amplifier,
    /// Flat +% elemental resistance to ALL elements (feeds player Resistances).
    Warding,
}

impl UpgradeId {
    /// Display order (also the buy-menu order). `COUNT` derives from this, so
    /// adding a variant only means a new entry here + its match arms.
    pub const ALL: [UpgradeId; 30] = [
        Self::HealthBoost,
        Self::ShieldBoost,
        Self::SpeedBoost,
        Self::SpareShip,
        Self::Multishot,
        Self::RapidFire,
        Self::Piercing,
        Self::BigShot,
        Self::StunShot,
        Self::HomingShot,
        Self::ExplodeShot,
        Self::KnockShot,
        Self::Vampirism,
        Self::Dodge,
        Self::CritChance,
        Self::CritDamage,
        Self::Thorns,
        Self::Regen,
        Self::LastStand,
        Self::Executioner,
        Self::PhaseEcho,
        Self::Overcharge,
        Self::StaticDischarge,
        Self::CombatMedic,
        Self::Momentum,
        Self::Whirlwind,
        Self::Catalyst,
        Self::Detonator,
        Self::Amplifier,
        Self::Warding,
    ];

    /// Total number of upgrades (sizes the `Upgrades` stack array).
    pub const COUNT: usize = Self::ALL.len();

    fn idx(self) -> usize {
        match self {
            Self::HealthBoost => 0,
            Self::ShieldBoost => 1,
            Self::SpeedBoost => 2,
            Self::SpareShip => 3,
            Self::Multishot => 4,
            Self::RapidFire => 5,
            Self::Piercing => 6,
            Self::BigShot => 7,
            Self::StunShot => 8,
            Self::HomingShot => 9,
            Self::ExplodeShot => 10,
            Self::KnockShot => 11,
            Self::Vampirism => 12,
            Self::Dodge => 13,
            Self::CritChance => 14,
            Self::CritDamage => 15,
            Self::Thorns => 16,
            Self::Regen => 17,
            Self::LastStand => 18,
            Self::Executioner => 19,
            Self::PhaseEcho => 20,
            Self::Overcharge => 21,
            Self::StaticDischarge => 22,
            Self::CombatMedic => 23,
            Self::Momentum => 24,
            Self::Whirlwind => 25,
            Self::Catalyst => 26,
            Self::Detonator => 27,
            Self::Amplifier => 28,
            Self::Warding => 29,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::HealthBoost => "Health Boost  (+35 max HP)",
            Self::ShieldBoost => "Shielding     (+8% DR)",
            Self::SpeedBoost => "Afterburner   (+thrust)",
            Self::SpareShip => "Spare Ship    (+1 tank)",
            Self::Multishot => "Multishot     (+1 shot)",
            Self::RapidFire => "Rapid Fire    (-12% cd)",
            Self::Piercing => "Piercing      (+1 pierce)",
            Self::BigShot => "Big Shot      (+2.2 radius)",
            Self::StunShot => "Stun Rounds   (+12% stun)",
            Self::HomingShot => "Homing Rounds (seek enemies)",
            Self::ExplodeShot => "Explosive     (+AoE on hit)",
            Self::KnockShot => "Knockback     (+15% shove)",
            Self::Vampirism => "Vampirism     (heal 5% dealt)",
            Self::Dodge => "Dodge         (+5% evade)",
            Self::CritChance => "Crit Chance   (+7%)",
            Self::CritDamage => "Crit Damage   (+15%)",
            Self::Thorns => "Thorns        (reflect 25%)",
            Self::Regen => "Repair Field  (+0.5 HP/s)",
            Self::LastStand => "Last Stand    (cheat death 1x)",
            Self::Executioner => "Executioner   (+20% vs <25% HP)",
            Self::PhaseEcho => "Phase Echo    (+2s dash invuln)",
            Self::Overcharge => "Overcharge    (every Nth shot ×3)",
            Self::StaticDischarge => "Static Disch. (periodic AoE)",
            Self::CombatMedic => "Combat Medic  (kill heals on hurt)",
            Self::Momentum => "Momentum      (+speed while moving)",
            Self::Whirlwind => "Whirlwind     (orbiting blade)",
            Self::Catalyst => "Catalyst      (+reaction dmg)",
            Self::Detonator => "Detonator     (+reaction radius)",
            Self::Amplifier => "Amplifier     (+12% damage)",
            Self::Warding => "Warding       (+8% all resist)",
        }
    }

    /// Pre-scale base cost (spec III.2 traits / III.5 DEFENSE costs / VIII.4).
    fn base_cost(self) -> u64 {
        match self {
            Self::HealthBoost => 1200,
            Self::ShieldBoost => 1500,
            Self::SpeedBoost => 2200,
            Self::SpareShip => 12000,
            Self::Multishot => 1800,
            Self::RapidFire => 1200,
            Self::Piercing => 1500,
            Self::BigShot => 1200,
            Self::StunShot => 1500,
            Self::HomingShot => 1600,
            Self::ExplodeShot => 1800,
            Self::KnockShot => 1300,
            Self::Vampirism => 2500,
            Self::Dodge => 1800,
            Self::CritChance => 2000,
            Self::CritDamage => 2000,
            Self::Thorns => 2200,
            Self::Regen => 2400,
            Self::LastStand => 8000,
            Self::Executioner => 2600,
            Self::PhaseEcho => 2000,
            Self::Overcharge => 1900,
            Self::StaticDischarge => 2300,
            Self::CombatMedic => 3000,
            Self::Momentum => 2000,
            Self::Whirlwind => 2500,
            Self::Catalyst => 2400,
            Self::Detonator => 2300,
            Self::Amplifier => 2000,
            Self::Warding => 2200,
        }
    }

    fn max_stacks(self) -> u32 {
        match self {
            Self::HealthBoost => 10,
            Self::ShieldBoost => 8,
            Self::SpeedBoost => 4,
            Self::SpareShip => 3,
            Self::Multishot => 3,
            Self::RapidFire => 4,
            Self::Piercing => 3,
            Self::BigShot => 3,
            Self::StunShot => 3,
            Self::HomingShot => 3,
            Self::ExplodeShot => 3,
            Self::KnockShot => 3,
            Self::Vampirism => 5,
            Self::Dodge => 10,
            Self::CritChance => 6,
            Self::CritDamage => 6,
            Self::Thorns => 4,
            Self::Regen => 6,
            Self::LastStand => 1,
            Self::Executioner => 5,
            Self::PhaseEcho => 2,
            Self::Overcharge => 4,
            Self::StaticDischarge => 5,
            Self::CombatMedic => 1,
            Self::Momentum => 4,
            Self::Whirlwind => 4,
            Self::Catalyst => 5,
            Self::Detonator => 4,
            Self::Amplifier => 5,
            Self::Warding => 5,
        }
    }
}

/// Elemental-reaction damage bonus from `Catalyst` stacks (+25%/stack): a
/// multiplier applied to shatter / oil-flare damage in `systems::reactions`.
pub fn catalyst_bonus(stacks: u32) -> f32 {
    0.25 * stacks as f32
}

/// Elemental-reaction *radius* bonus from `Detonator` stacks (+20%/stack):
/// scales the shatter / oil-flare blast radius in `systems::reactions`.
pub fn detonator_bonus(stacks: u32) -> f32 {
    0.20 * stacks as f32
}

/// All-weapon damage multiplier from `Amplifier` stacks (+12%/stack): applied to
/// player bullet damage in `collision::bullet_hits_enemy`.
pub fn amplifier_mult(stacks: u32) -> f32 {
    1.0 + 0.12 * stacks as f32
}

/// All-element resist fraction from `Warding` stacks (+8%/stack): added to the
/// player's per-element `Resistances` in `items::apply_item_resist`.
pub fn warding_bonus(stacks: u32) -> f32 {
    0.08 * stacks as f32
}

/// Stun chance applied by the `_STUN` bullet trait at `stacks` (spec III.6:
/// `0.12 × stacks`).
pub fn stun_chance(stacks: u32) -> f32 {
    0.12 * stacks as f32
}

/// `_HOMING` trait turn rate (rad/sec): `min(0.4, 0.09×stacks)` rad/*frame* at
/// the JS 60 Hz tick → ×60 for our seconds-based steering (spec III.2). 0 = off.
pub fn homing_turn_rate(stacks: u32) -> f32 {
    if stacks == 0 {
        0.0
    } else {
        (0.09 * stacks as f32).min(0.4) * 60.0
    }
}

/// `_EXPLODE` trait blast radius: `30 + 10×stacks` px (spec III.2). 0 = off.
pub fn explosion_radius(stacks: u32) -> f32 {
    if stacks == 0 {
        0.0
    } else {
        30.0 + 10.0 * stacks as f32
    }
}

/// `_KNOCK` trait shove chance at `stacks` (spec III.6: `0.15 × stacks`); the
/// proc itself is a flat 16 px shove (`KNOCK_PX`).
pub fn knock_chance(stacks: u32) -> f32 {
    0.15 * stacks as f32
}

/// Flat positional shove distance for a `_KNOCK` proc (spec III.6).
pub const KNOCK_PX: f32 = 16.0;

/// VAMPIRISM passive: heal `0.05 × stacks` of damage dealt (spec III.5, ×5).
pub fn vampirism_frac(stacks: u32) -> f32 {
    0.05 * stacks as f32
}

/// DODGE passive: chance to ignore a hit, `min(0.5, 0.05 × stacks)` (spec III.5/II.2).
pub fn dodge_chance(stacks: u32) -> f32 {
    (0.05 * stacks as f32).min(0.5)
}

/// THORNS passive: fraction of taken damage reflected to the attacker,
/// `0.25 × stacks` (spec III.5, ×4).
pub fn thorns_frac(stacks: u32) -> f32 {
    0.25 * stacks as f32
}

/// EXECUTIONER passive (spec VI.3): bonus *damage multiplier* added vs an enemy
/// below the execute threshold — `0.20 × stacks` (so a hit deals `×(1 + bonus)`).
pub fn executioner_bonus(stacks: u32) -> f32 {
    0.20 * stacks as f32
}

/// HP fraction below which `executioner_bonus` applies (spec VI.3: <25%).
pub const EXECUTE_THRESHOLD: f32 = 0.25;

/// PHASE ECHO passive (spec VI.3): extra post-dash invulnerability, `2.0 ×
/// stacks` seconds added on top of the dash's base i-frames.
pub fn phase_echo_secs(stacks: u32) -> f32 {
    2.0 * stacks as f32
}

/// OVERCHARGE ROUNDS passive (spec VI.3): every Nth bullet deals ×3. Returns the
/// interval N, smaller (more frequent) with more stacks; `0` = off (no stacks).
/// (Spec gives "every Nth bullet ×3"; the exact N is interpreted as `8 − stacks`,
/// i.e. 1→7th … 4→4th.)
pub fn overcharge_interval(stacks: u32) -> u32 {
    if stacks == 0 {
        0
    } else {
        8 - stacks.min(4)
    }
}

/// STATIC DISCHARGE passive (spec VI.3): seconds between AoE pulses, faster with
/// more stacks (`max(1.0, 2.0 − 0.2×stacks)`). Only meaningful when owned.
pub fn static_discharge_interval(stacks: u32) -> f32 {
    (2.0 - 0.2 * stacks as f32).max(1.0)
}

/// STATIC DISCHARGE damage per pulse: `4.0 × stacks` (0 = off).
pub fn static_discharge_damage(stacks: u32) -> f32 {
    4.0 * stacks as f32
}

/// WHIRLWIND passive (spec VI.3): the orbiting damage zone's DPS, `8.0 × stacks`
/// (0 = off). Applied continuously (`dps × dt`) to enemies in the zone.
pub fn whirlwind_dps(stacks: u32) -> f32 {
    8.0 * stacks as f32
}

/// MOMENTUM passive (spec VI.3): a speed bonus that ramps with `sustained`
/// seconds of continuous movement — `+5%/s × stacks`, capped at `+15% × stacks`.
/// Returns the *fraction* added to top speed (0 when unowned or at rest).
pub fn momentum_bonus(sustained: f32, stacks: u32) -> f32 {
    if stacks == 0 {
        return 0.0;
    }
    let s = stacks as f32;
    (0.05 * s * sustained).min(0.15 * s)
}

/// Passive-regen rate HP/s with `Regen` stacks (spec II.2: `min(3.0, 0.5×stacks)`).
pub fn regen_rate(stacks: u32) -> f32 {
    (0.5 * stacks as f32).min(3.0)
}

/// Seconds without damage before passive regen begins (spec II.2: 4000 ms).
pub const REGEN_DELAY: f32 = 4.0;

/// Owned stack counts, indexed by `UpgradeId`. Run-scoped (reset per run).
#[derive(Resource, Default)]
pub struct Upgrades {
    owned: [u32; UpgradeId::COUNT],
}

impl Upgrades {
    pub fn owned(&self, id: UpgradeId) -> u32 {
        self.owned[id.idx()]
    }
    pub fn inc(&mut self, id: UpgradeId) {
        self.owned[id.idx()] += 1;
    }
    /// Set an upgrade's owned stacks directly (used by tests / debug).
    pub fn set(&mut self, id: UpgradeId, count: u32) {
        self.owned[id.idx()] = count;
    }
    /// Reset all stacks (called at the start of a fresh run).
    pub fn reset(&mut self) {
        self.owned = [0; UpgradeId::COUNT];
    }
}

/// Current shop menu selection index.
#[derive(Resource, Default)]
pub struct ShopSel(pub usize);

/// Cost of the *next* stack given `owned` already-bought stacks (spec VIII.4):
/// `base × 13 × 1.6^owned`, rounded to the nearest 500, floored at 500.
pub fn upgrade_cost(base: u64, owned: u32) -> u64 {
    let raw = base as f64 * 13.0 * 1.6_f64.powi(owned as i32);
    (((raw / 500.0).round() * 500.0) as u64).max(500)
}

// ── UI ───────────────────────────────────────────────────────────────────────

/// Marks the shop overlay root (despawned on exit via `flow::despawn_screen`).
#[derive(Component)]
pub struct ShopPanel;

/// Marks the live shop text (rebuilt each frame).
#[derive(Component)]
pub struct ShopText;

pub fn spawn_shop_ui(mut commands: Commands) {
    commands
        .spawn((
            ShopPanel,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.03, 0.82)),
        ))
        .with_children(|p| {
            p.spawn((
                ShopText,
                Text::new(""),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
            ));
        });
}

/// Rebuild the shop text from gold + owned stacks + selection each frame.
pub fn shop_ui_update(
    score: Res<Score>,
    upgrades: Res<Upgrades>,
    sel: Res<ShopSel>,
    mut q: Query<&mut Text, With<ShopText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let mut s = format!("=== SHOP ===          GOLD: {}\n\n", score.gold);
    for (i, id) in UpgradeId::ALL.iter().enumerate() {
        let owned = upgrades.owned(*id);
        let cursor = if i == sel.0 { ">" } else { " " };
        let cost_label = if owned >= id.max_stacks() {
            "MAX".to_string()
        } else {
            let cost = upgrade_cost(id.base_cost(), owned);
            let afford = if score.gold >= cost { "" } else { "  (need more)" };
            format!("{cost}g{afford}")
        };
        s.push_str(&format!(
            "{cursor} {:<28} x{}/{}   {}\n",
            id.name(),
            owned,
            id.max_stacks(),
            cost_label
        ));
    }
    s.push_str("\n[ Up/Down select   ENTER buy   B/ESC close ]");
    if text.0 != s {
        text.0 = s;
    }
}

// ── Open / close + buy ─────────────────────────────────────────────────────

/// **B** in play opens the shop (resetting the selection to the top).
pub fn open_shop(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<ShopSel>,
) {
    if keys.just_pressed(KeyCode::KeyB) {
        sel.0 = 0;
        next.set(GameState::Shop);
    }
}

/// Navigate (Up/Down or W/S), buy (ENTER/Space), close (B/Esc).
pub fn shop_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<ShopSel>,
    mut score: ResMut<Score>,
    mut upgrades: ResMut<Upgrades>,
    mut player: Query<(&mut Health, &mut Shield, &mut Ship, &mut Lives), With<Ship>>,
) {
    let n = UpgradeId::ALL.len();

    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyB) {
        next.set(GameState::Playing);
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        sel.0 = (sel.0 + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        sel.0 = (sel.0 + 1) % n;
    }

    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        let id = UpgradeId::ALL[sel.0];
        let owned = upgrades.owned(id);
        if owned >= id.max_stacks() {
            return; // maxed
        }
        let cost = upgrade_cost(id.base_cost(), owned);
        if score.gold < cost {
            return; // can't afford
        }
        score.gold -= cost;
        upgrades.inc(id);

        if let Ok((mut hp, mut shield, mut ship, mut lives)) = player.single_mut() {
            apply_upgrade(id, &mut hp, &mut shield, &mut ship, &mut lives);
        }
    }
}

/// Apply an acquired upgrade's effect to the ship (shop purchase or survivor
/// card). Stat upgrades mutate the ship; weapon/passive traits are no-ops here
/// (their effect is read live in combat).
pub fn apply_upgrade(
    id: UpgradeId,
    hp: &mut Health,
    shield: &mut Shield,
    ship: &mut Ship,
    lives: &mut Lives,
) {
    match id {
        UpgradeId::HealthBoost => {
            // +35 max HP (cap 600, spec II.2) + full heal.
            hp.max = (hp.max + 35.0).min(600.0);
            hp.current = hp.max;
        }
        UpgradeId::ShieldBoost => {
            shield.reduction = (shield.reduction + 0.08).min(SHIELD_REDUCTION_CAP);
        }
        UpgradeId::SpeedBoost => {
            // "+65% thrust" — applied as a fixed additive bump off the base.
            ship.thrust += 700.0;
            ship.max_speed += 120.0;
        }
        UpgradeId::SpareShip => {
            lives.count += 1;
        }
        // Primary-weapon traits take effect live in the weapon systems (they read
        // the Upgrades resource each shot), so buying them is just the stack bump.
        UpgradeId::Multishot
        | UpgradeId::RapidFire
        | UpgradeId::Piercing
        | UpgradeId::BigShot
        | UpgradeId::StunShot
        | UpgradeId::HomingShot
        | UpgradeId::ExplodeShot
        | UpgradeId::KnockShot
        | UpgradeId::Vampirism
        | UpgradeId::Dodge
        | UpgradeId::CritChance
        | UpgradeId::CritDamage
        | UpgradeId::Thorns
        | UpgradeId::Regen
        | UpgradeId::LastStand
        | UpgradeId::Executioner
        | UpgradeId::PhaseEcho
        | UpgradeId::Overcharge
        | UpgradeId::StaticDischarge
        | UpgradeId::CombatMedic
        | UpgradeId::Momentum
        | UpgradeId::Whirlwind
        | UpgradeId::Catalyst
        | UpgradeId::Detonator
        | UpgradeId::Amplifier
        | UpgradeId::Warding => {}
    }
}
