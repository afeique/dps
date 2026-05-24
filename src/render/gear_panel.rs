//! Equipped-gear panel (spec VI.5 / VIII.1) — a compact right-edge HUD column
//! showing the player's five equipment slots and what's slotted in each, tinted
//! by rarity. Pure presentation: reads the [`Equipment`] resource each frame and
//! writes one `Text` line per slot. The left-edge `loot_feed` announces drops;
//! this panel is the persistent "current build" view.

use crate::systems::items::{Equipment, Item, ItemSlot};
use bevy::prelude::*;

/// One gear-panel row, bound to the slot it displays.
#[derive(Component, Clone, Copy)]
pub struct GearRow(pub ItemSlot);

/// Dim color for an empty slot.
const EMPTY_COLOR: Color = Color::srgb(0.34, 0.36, 0.42);

/// One row's text: the slot label + the equipped item's name, or a dash when
/// empty. Pure (unit-tested).
pub fn gear_row_text(slot: ItemSlot, item: Option<&Item>) -> String {
    match item {
        Some(it) => format!("{:<9} {}", slot.label(), it.name),
        None => format!("{:<9} —", slot.label()),
    }
}

/// Spawn the gear column (header + one row per slot) once at startup.
pub fn setup_gear_panel(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: Val::Px(12.0),
            top: Val::Percent(32.0),
            width: Val::Px(214.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn((
                Text::new("GEAR"),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::srgb(0.7, 0.8, 0.9)),
            ));
            for slot in ItemSlot::ALL {
                col.spawn((
                    GearRow(slot),
                    Text::new(gear_row_text(slot, None)),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(EMPTY_COLOR),
                ));
            }
        });
}

/// Refresh each gear row from the current equipment each frame: an equipped
/// slot shows its item name in the rarity color; an empty slot stays dim.
pub fn update_gear_panel(
    equipment: Res<Equipment>,
    mut rows: Query<(&GearRow, &mut Text, &mut TextColor)>,
) {
    for (row, mut text, mut color) in &mut rows {
        let item = equipment.get(row.0);
        let s = gear_row_text(row.0, item);
        if text.0 != s {
            text.0 = s;
        }
        color.0 = item.map_or(EMPTY_COLOR, |it| it.rarity.color());
    }
}
