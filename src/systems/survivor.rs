//! Survivor-card wave-clear flow (spec V.6 / III.5 `PASSIVE_REWARD_IDS`).
//!
//! When a non-final wave clears, `wave::spawn_waves` sets `Wave.awaiting_reward`;
//! `check_survivor` opens the `Survivor` state (which pauses the sim) offering 3
//! random cards from the passive pool. Pressing 1/2/3 grants the card (+ a
//! wave-scaled coin bonus), advances the wave, and resumes `Playing`.
//!
//! Cards = the spec passive pool; stat cards (Health/Shield/Speed) apply to the
//! ship immediately, the rest are read live in combat. Delivery here supersedes
//! the shop stand-in for these passives.

use crate::components::{Health, Lives, Ship, Shield};
use crate::resources::{GameRng, Score};
use crate::states::GameState;
use crate::systems::shop::{apply_upgrade, UpgradeId, Upgrades};
use crate::systems::wave::Wave;
use bevy::prelude::*;

/// The wave-clear card pool (`PASSIVE_REWARD_IDS`, spec III.5).
pub const POOL: [UpgradeId; 23] = [
    UpgradeId::CritChance,
    UpgradeId::CritDamage,
    UpgradeId::HealthBoost,
    UpgradeId::ShieldBoost,
    UpgradeId::Vampirism,
    UpgradeId::Thorns,
    UpgradeId::Dodge,
    UpgradeId::SpeedBoost,
    UpgradeId::Catalyst,
    UpgradeId::Detonator,
    UpgradeId::Amplifier,
    UpgradeId::Warding,
    UpgradeId::Predator,
    UpgradeId::Opportunist,
    UpgradeId::Bloodlust,
    UpgradeId::VampiricRounds,
    UpgradeId::Ricochet,
    UpgradeId::Berserker,
    UpgradeId::Magnetism,
    UpgradeId::Frenzy,
    UpgradeId::Scavenger,
    UpgradeId::XpBoost,
    UpgradeId::Overflow,
];

/// The three cards currently on offer.
#[derive(Resource, Default)]
pub struct SurvivorChoice {
    pub cards: [Option<UpgradeId>; 3],
}

/// Marks the survivor overlay root.
#[derive(Component)]
pub struct SurvivorScreen;

/// Marks the live survivor text (rebuilt each frame).
#[derive(Component)]
pub struct SurvivorText;

/// Pick 3 distinct cards from `POOL` using the shared RNG (deterministic given a
/// seed; assert on properties, not the exact draw).
pub fn choose_cards(rng: &mut GameRng) -> [Option<UpgradeId>; 3] {
    let mut pool: Vec<UpgradeId> = POOL.to_vec();
    let mut out = [None; 3];
    for slot in out.iter_mut() {
        if pool.is_empty() {
            break;
        }
        let i = (rng.next_f32() * pool.len() as f32) as usize;
        let i = i.min(pool.len() - 1);
        *slot = Some(pool.remove(i));
    }
    out
}

/// A stage clear is every 3rd wave (3, 6, 9, … — the boss waves, spec V.6).
pub fn is_stage_clear(wave_n: u64) -> bool {
    wave_n % 3 == 0
}

/// Mid-stage clear bonus coins (spec V.6: `round((50+wave*25)*0.6)`).
pub fn midstage_bonus(wave_n: u64) -> u64 {
    ((50 + wave_n * 25) as f32 * 0.6).round() as u64
}

/// Stage-clear bonus coins (spec V.6: `(50+wave*25)*2`).
pub fn stage_bonus(wave_n: u64) -> u64 {
    (50 + wave_n * 25) * 2
}

/// While `Playing`, react to a wave clear (`awaiting_reward`): on a **stage
/// clear** open the survivor pick; on a **mid-stage** wave auto-advance with a
/// smaller coin bonus and no pick (spec V.6).
pub fn check_survivor(
    mut wave: ResMut<Wave>,
    mut score: ResMut<Score>,
    mut next: ResMut<NextState<GameState>>,
) {
    if !wave.awaiting_reward {
        return;
    }
    if is_stage_clear(wave.number() as u64) {
        next.set(GameState::Survivor); // the pick (+ bonus + advance) happens there
    } else {
        score.gold = score
            .gold
            .saturating_add(midstage_bonus(wave.number() as u64));
        wave.advance_after_reward();
    }
}

/// On entering `Survivor`: roll the three offered cards + spawn the overlay.
pub fn enter_survivor(
    mut commands: Commands,
    mut rng: ResMut<GameRng>,
    mut choice: ResMut<SurvivorChoice>,
) {
    choice.cards = choose_cards(&mut rng);
    commands
        .spawn((
            SurvivorScreen,
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
            BackgroundColor(Color::srgba(0.0, 0.02, 0.04, 0.85)),
        ))
        .with_children(|p| {
            p.spawn((
                SurvivorText,
                Text::new(""),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(Color::srgb(0.7, 1.0, 0.85)),
            ));
        });
}

/// Rebuild the survivor card text each frame.
pub fn survivor_ui_update(
    choice: Res<SurvivorChoice>,
    mut q: Query<&mut Text, With<SurvivorText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let mut s = String::from("WAVE CLEARED — choose a survivor card:\n\n");
    for (i, card) in choice.cards.iter().enumerate() {
        if let Some(id) = card {
            s.push_str(&format!("  [{}] {}\n", i + 1, id.name()));
        }
    }
    s.push_str("\nPress 1 / 2 / 3");
    if text.0 != s {
        text.0 = s;
    }
}

/// 1/2/3 grants the chosen card (+ coin bonus), advances the wave, resumes play.
pub fn survivor_input(
    keys: Res<ButtonInput<KeyCode>>,
    choice: Res<SurvivorChoice>,
    mut wave: ResMut<Wave>,
    mut upgrades: ResMut<Upgrades>,
    mut score: ResMut<Score>,
    mut next: ResMut<NextState<GameState>>,
    mut player: Query<(&mut Health, &mut Shield, &mut Ship, &mut Lives), With<Ship>>,
) {
    let pick = if keys.just_pressed(KeyCode::Digit1) {
        0
    } else if keys.just_pressed(KeyCode::Digit2) {
        1
    } else if keys.just_pressed(KeyCode::Digit3) {
        2
    } else {
        return;
    };
    let Some(id) = choice.cards.get(pick).copied().flatten() else {
        return;
    };

    upgrades.inc(id);
    if let Ok((mut hp, mut shield, mut ship, mut lives)) = player.single_mut() {
        apply_upgrade(id, &mut hp, &mut shield, &mut ship, &mut lives);
    }

    // The survivor pick only fires on stage clears, so the bonus is the doubled
    // stage-clear amount (spec V.6).
    score.gold = score
        .gold
        .saturating_add(stage_bonus(wave.number() as u64));

    wave.advance_after_reward();
    // Chain into the shop (spec V.6): a stage-clear pick flows into a "spend your
    // gold" break before the next wave. Closing the shop (Esc/B) resumes Playing →
    // the next wave. (The spec's curated 3-cheapest-suggest is shown as the full
    // shop here; the curation is a refinement.)
    next.set(GameState::Shop);
}
