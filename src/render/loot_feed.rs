//! Left-edge **loot feed** (spec VI.5 / VIII.1) — the cards that announce item
//! drops. Pure presentation: it drains the [`LootFeed`] resource (filled by
//! `systems::items::roll_item_drops_on_death`) into a flex column of cards on the
//! left edge, each tinted by rarity, then ages them out after a short window.
//!
//! Mirrors the `wave_title` transient-overlay pattern (spawn → live for N
//! seconds → despawn) but stacks via a flex container so expiring cards reflow
//! cleanly. No equip / stat effect yet — the cards are the visible half of the
//! VI.5 first slice.

use crate::systems::items::{Item, LootFeed};
use bevy::prelude::*;

/// The flex-column container all loot cards live under (spawned once).
#[derive(Component)]
pub struct LootFeedRoot;

/// A live loot card; despawns (with its text children) when `life` hits zero.
#[derive(Component)]
pub struct LootCard {
    life: f32,
}

/// Seconds a card stays on screen before it despawns.
const CARD_LIFE: f32 = 6.0;
/// Cap on simultaneous cards — excess oldest cards are retired early so a kill
/// spree can't bury the screen.
const MAX_CARDS: usize = 6;

/// Spawn the empty left-edge column once at startup.
pub fn setup_loot_feed(mut commands: Commands) {
    commands.spawn((
        LootFeedRoot,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Percent(38.0),
            width: Val::Px(232.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            ..default()
        },
    ));
}

/// Drain newly-minted items into cards appended to the feed column.
pub fn drain_loot_feed(
    mut commands: Commands,
    mut feed: ResMut<LootFeed>,
    root: Query<Entity, With<LootFeedRoot>>,
) {
    if feed.pending.is_empty() {
        return;
    }
    let Ok(root) = root.single() else {
        // No feed container (e.g. headless / not yet set up) — drop the backlog
        // rather than let it grow.
        feed.pending.clear();
        return;
    };
    let items: Vec<Item> = std::mem::take(&mut feed.pending);
    commands.entity(root).with_children(|col| {
        for item in items {
            let glow = item.rarity.color();
            let accent = item.slot.accent();
            col.spawn((
                LootCard { life: CARD_LIFE },
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(6.0)),
                    border: UiRect::left(Val::Px(3.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.04, 0.05, 0.09, 0.72)),
                BorderColor::all(glow),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(item.name.clone()),
                    TextFont { font_size: 14.0, ..default() },
                    TextColor(glow),
                ));
                card.spawn((
                    Text::new(item.affix_summary()),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(accent),
                ));
            });
        }
    });
}

/// Age cards and despawn the expired; also retire the oldest if we exceed the
/// on-screen cap (the lowest `life` is the oldest, since all start at `CARD_LIFE`).
pub fn age_loot_cards(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut LootCard)>,
) {
    let dt = time.delta_secs();
    for (_, mut card) in &mut q {
        card.life -= dt;
    }

    // Collect surviving cards (entity, life), retire any past end-of-life.
    let mut alive: Vec<(Entity, f32)> = Vec::new();
    for (e, card) in &q {
        if card.life <= 0.0 {
            commands.entity(e).despawn();
        } else {
            alive.push((e, card.life));
        }
    }

    // Enforce the cap: drop the oldest (smallest life) beyond MAX_CARDS.
    if alive.len() > MAX_CARDS {
        alive.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for &(e, _) in &alive[..alive.len() - MAX_CARDS] {
            commands.entity(e).despawn();
        }
    }
}
