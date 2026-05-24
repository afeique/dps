//! Left-edge **loot feed** (spec VI.5 / VIII.1) — the cards that announce item
//! drops. Pure presentation: it drains the [`LootFeed`] resource (filled by
//! `systems::items::roll_item_drops_on_death`) into a flex column of cards on the
//! left edge, each tinted by rarity, then ages them out after a short window.
//!
//! Mirrors the `wave_title` transient-overlay pattern (spawn → live for N
//! seconds → despawn) but stacks via a flex container so expiring cards reflow
//! cleanly. No equip / stat effect yet — the cards are the visible half of the
//! VI.5 first slice.

use crate::systems::items::{LootEntry, LootFeed};
use bevy::prelude::*;

/// The flex-column container all loot cards live under (spawned once).
#[derive(Component)]
pub struct LootFeedRoot;

/// A live loot card; fades over its last `FADE_SECS` then despawns (with its
/// text children) when `life` hits zero. Stores its rarity `glow` + slot
/// `accent` so the fade can re-tint the border + text each frame.
#[derive(Component)]
pub struct LootCard {
    life: f32,
    glow: Color,
    accent: Color,
}

/// Seconds a card stays on screen before it despawns.
const CARD_LIFE: f32 = 6.0;
/// The card fades to transparent over its final `FADE_SECS`.
const FADE_SECS: f32 = 1.2;
/// Card-panel background base alpha (scaled by the fade).
const BG_ALPHA: f32 = 0.72;
/// Cap on simultaneous cards — excess oldest cards are retired early so a kill
/// spree can't bury the screen.
const MAX_CARDS: usize = 6;

/// Opacity (0..1) for a card with `life` seconds left: full until the last
/// `FADE_SECS`, then a linear fade to 0.
pub fn card_alpha(life: f32) -> f32 {
    (life / FADE_SECS).clamp(0.0, 1.0)
}

/// The card's title line: an auto-equipped drop gets a ▲ upgrade marker; a
/// sidegrade (kept-out-of-the-slot) shows the name plainly.
pub fn card_title(name: &str, equipped: bool) -> String {
    if equipped {
        format!("▲ {name}")
    } else {
        name.to_string()
    }
}

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
    let entries: Vec<LootEntry> = std::mem::take(&mut feed.pending);
    commands.entity(root).with_children(|col| {
        for LootEntry { item, equipped } in entries {
            let glow = item.rarity.color();
            let accent = item.slot.accent();
            col.spawn((
                LootCard { life: CARD_LIFE, glow, accent },
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
                    Text::new(card_title(&item.name, equipped)),
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

/// Age cards: tick life, fade the panel + border + text over the last
/// `FADE_SECS`, despawn the expired, and retire the oldest beyond the on-screen
/// cap (lowest `life` is oldest, since all start at `CARD_LIFE`).
pub fn age_loot_cards(
    time: Res<Time>,
    mut commands: Commands,
    mut cards: Query<(
        Entity,
        &mut LootCard,
        &Children,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut texts: Query<&mut TextColor>,
) {
    let dt = time.delta_secs();
    let mut alive: Vec<(Entity, f32)> = Vec::new();

    for (e, mut card, children, mut bg, mut border) in &mut cards {
        card.life -= dt;
        if card.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let a = card_alpha(card.life);
        bg.0.set_alpha(BG_ALPHA * a);
        *border = BorderColor::all(card.glow.with_alpha(a));
        // Children, in spawn order: [0] name (glow), [1] affixes (accent).
        for (k, child) in children.iter().enumerate() {
            if let Ok(mut tc) = texts.get_mut(child) {
                let base = if k == 0 { card.glow } else { card.accent };
                tc.0 = base.with_alpha(a);
            }
        }
        alive.push((e, card.life));
    }

    // Enforce the cap: drop the oldest (smallest life) beyond MAX_CARDS.
    if alive.len() > MAX_CARDS {
        alive.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for &(e, _) in &alive[..alive.len() - MAX_CARDS] {
            commands.entity(e).despawn();
        }
    }
}
