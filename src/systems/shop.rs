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
}

impl UpgradeId {
    /// Display order (also the buy-menu order).
    pub const ALL: [UpgradeId; 8] = [
        Self::HealthBoost,
        Self::ShieldBoost,
        Self::SpeedBoost,
        Self::SpareShip,
        Self::Multishot,
        Self::RapidFire,
        Self::Piercing,
        Self::BigShot,
    ];

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
        }
    }
}

/// Owned stack counts, indexed by `UpgradeId`. Run-scoped (reset per run).
#[derive(Resource, Default)]
pub struct Upgrades {
    owned: [u32; 8],
}

impl Upgrades {
    pub fn owned(&self, id: UpgradeId) -> u32 {
        self.owned[id.idx()]
    }
    fn inc(&mut self, id: UpgradeId) {
        self.owned[id.idx()] += 1;
    }
    /// Reset all stacks (called at the start of a fresh run).
    pub fn reset(&mut self) {
        self.owned = [0; 8];
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
        | UpgradeId::BigShot => {}
    }
}
