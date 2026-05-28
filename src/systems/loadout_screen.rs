//! The LOADOUT picker (Phase AB/ME): assign abilities to the 4 ability slots
//! from the pool of available (base + armory-unlocked) abilities. Reached with
//! `L` from the title (its own `GameState::Loadout`); `Esc` returns. Edits the
//! `EquippedAbilities` resource the run reads (session-scoped). Native-UI overlay
//! mirroring the armory / skill-points screens.

use crate::components::loadout::{cycle_slot_ability, EquippedAbilities};
use crate::meta::Meta;
use crate::states::GameState;
use bevy::prelude::*;

/// Cursor over the 4 ability slots.
#[derive(Resource, Default)]
pub struct LoadoutSel(pub usize);

#[derive(Component)]
pub struct LoadoutPanel;

#[derive(Component)]
pub struct LoadoutText;

/// `L` on the title screen opens the loadout picker (resetting the cursor).
pub fn open_loadout(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<LoadoutSel>,
) {
    if keys.just_pressed(KeyCode::KeyL) {
        sel.0 = 0;
        next.set(GameState::Loadout);
    }
}

pub fn spawn_loadout_ui(mut commands: Commands) {
    commands
        .spawn((
            LoadoutPanel,
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
            BackgroundColor(Color::srgba(0.0, 0.04, 0.04, 0.92)),
        ))
        .with_children(|p| {
            p.spawn((
                LoadoutText,
                Text::new(""),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(0.7, 1.0, 0.85)),
            ));
        });
}

/// Rebuild the loadout text: the 4 slots with their equipped ability + cursor.
pub fn loadout_ui_update(
    equipped: Res<EquippedAbilities>,
    sel: Res<LoadoutSel>,
    mut q: Query<&mut Text, With<LoadoutText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let mut s = String::from("=== LOADOUT ===   (4 ability slots, keys Numpad 1-4)\n\n");
    for slot in 0..4usize {
        let cursor = if slot == sel.0 { ">" } else { " " };
        let name = match equipped.0[slot] {
            Some(a) => a.name(),
            None => "— empty —",
        };
        s.push_str(&format!("{cursor} Slot {}   {}\n", slot + 1, name));
    }
    s.push_str("\n[ Up/Down slot   Left/Right change   ESC back ]");
    s.push_str("\n(unlock more abilities in the ARMORY)");
    if text.0 != s {
        text.0 = s;
    }
}

/// Navigate slots (Up/Down), change the selected slot's ability (Left/Right
/// cycles through the available pool + empty), back (Esc).
pub fn loadout_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<LoadoutSel>,
    meta: Res<Meta>,
    mut equipped: ResMut<EquippedAbilities>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Title);
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        sel.0 = (sel.0 + 3) % 4;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        sel.0 = (sel.0 + 1) % 4;
    }
    let slot = sel.0.min(3);
    if keys.just_pressed(KeyCode::ArrowRight) {
        equipped.0[slot] = cycle_slot_ability(equipped.0[slot], &meta, true);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        equipped.0[slot] = cycle_slot_ability(equipped.0[slot], &meta, false);
    }
}
