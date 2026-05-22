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
}

impl UpgradeId {
    /// Display order (also the buy-menu order). `COUNT` derives from this, so
    /// adding a variant only means a new entry here + its match arms.
    pub const ALL: [UpgradeId; 16] = [
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
        }
    }

    fn name(self) -> &'static str {
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
        }
    }
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

/// Owned stack counts, indexed by `UpgradeId`. Run-scoped (reset per run).
#[derive(Resource, Default)]
pub struct Upgrades {
    owned: [u32; UpgradeId::COUNT],
}

impl Upgrades {
    pub fn owned(&self, id: UpgradeId) -> u32 {
        self.owned[id.idx()]
    }
    fn inc(&mut self, id: UpgradeId) {
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

/// Apply a purchased upgrade's effect to the ship.
fn apply_upgrade(
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
        | UpgradeId::CritDamage => {}
    }
}
