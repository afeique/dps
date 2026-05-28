//! The ARMORY (Phase ME): spend persistent **account-gold** on permanent
//! unlocks. Reached with `A` from the title (its own `GameState::Armory`); `Esc`
//! returns. A native-UI overlay mirroring the shop. Unlocks are recorded on
//! [`Meta`] and persisted via [`save_meta`]; the run loadout reads them through
//! `WeaponKind::is_available`. The base loadout is free and not listed here.
//!
//! v1 lists the six exotic weapons; abilities/attunements join the catalog as
//! their own unlock-gating lands.

use crate::combat::element::Element;
use crate::components::loadout::Ability;
use crate::meta::{save_meta, Meta, ABILITY_UNLOCK_COST, ATTUNEMENT_UNLOCK_COST, WEAPON_UNLOCK_COST};
use crate::states::GameState;
use crate::systems::weapons::{attunement_unlock_id, WeaponKind};
use bevy::prelude::*;

/// One purchasable unlock: a stable id (matches `Meta.unlocked`), a display
/// name, and an account-gold cost.
pub struct ArmoryEntry {
    pub id: &'static str,
    pub name: String,
    pub cost: u64,
}

/// The unlock catalog — the six exotic weapons + the six elemental attunements
/// (the five base weapons + "no attunement" are free and omitted). Abilities
/// append here as their gating lands.
pub fn armory_catalog() -> Vec<ArmoryEntry> {
    const EXOTICS: [WeaponKind; 6] = [
        WeaponKind::GravityLance,
        WeaponKind::SpinCannon,
        WeaponKind::Boomerang,
        WeaponKind::Caroms,
        WeaponKind::MitosisRounds,
        WeaponKind::FlakCannon,
    ];
    const ATTUNES: [Element; 6] = [
        Element::Pyro,
        Element::Cryo,
        Element::Volt,
        Element::Toxic,
        Element::Void,
        Element::Radiant,
    ];
    let weapons = EXOTICS.iter().map(|w| ArmoryEntry {
        id: w.id(),
        name: w.name().to_string(),
        cost: WEAPON_UNLOCK_COST,
    });
    let attunes = ATTUNES.iter().map(|e| ArmoryEntry {
        id: attunement_unlock_id(*e),
        name: format!("{} Attunement", e.name()),
        cost: ATTUNEMENT_UNLOCK_COST,
    });
    // The non-base abilities (the four default-loadout ones are free).
    let abilities = Ability::ALL
        .iter()
        .filter(|a| !a.base_unlocked())
        .map(|a| ArmoryEntry {
            id: a.id(),
            name: a.name().to_string(),
            cost: ABILITY_UNLOCK_COST,
        });
    weapons.chain(attunes).chain(abilities).collect()
}

/// Cursor over the armory catalog.
#[derive(Resource, Default)]
pub struct ArmorySel(pub usize);

/// Root of the armory overlay (despawned on exit via `flow::despawn_screen`).
#[derive(Component)]
pub struct ArmoryPanel;

/// The live armory text (rebuilt each frame).
#[derive(Component)]
pub struct ArmoryText;

/// `A` on the title screen opens the armory (resetting the cursor).
pub fn open_armory(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<ArmorySel>,
) {
    if keys.just_pressed(KeyCode::KeyA) {
        sel.0 = 0;
        next.set(GameState::Armory);
    }
}

pub fn spawn_armory_ui(mut commands: Commands) {
    commands
        .spawn((
            ArmoryPanel,
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
            BackgroundColor(Color::srgba(0.03, 0.0, 0.05, 0.92)),
        ))
        .with_children(|p| {
            p.spawn((
                ArmoryText,
                Text::new(""),
                TextFont {
                    // Small enough to fit the full catalog (weapons + attunements
                    // + abilities) without clipping.
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.88, 0.45)),
            ));
        });
}

/// Rebuild the armory text from the account wallet + unlock state + cursor.
pub fn armory_ui_update(
    meta: Res<Meta>,
    sel: Res<ArmorySel>,
    mut q: Query<&mut Text, With<ArmoryText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let catalog = armory_catalog();
    let mut s = format!("=== ARMORY ===        ACCOUNT GOLD: {}\n\n", meta.account_gold);
    for (i, e) in catalog.iter().enumerate() {
        let cursor = if i == sel.0 { ">" } else { " " };
        let status = if meta.is_unlocked(e.id) {
            "UNLOCKED".to_string()
        } else if meta.account_gold >= e.cost {
            format!("{}g", e.cost)
        } else {
            format!("{}g  (need more)", e.cost)
        };
        s.push_str(&format!("{cursor} {:<20} {}\n", e.name, status));
    }
    s.push_str("\n[ Up/Down select   ENTER unlock   ESC back ]");
    if text.0 != s {
        text.0 = s;
    }
}

/// Navigate (Up/Down or W/S), unlock the selected item (ENTER), back (Esc).
pub fn armory_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<ArmorySel>,
    mut meta: ResMut<Meta>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Title);
        return;
    }
    let catalog = armory_catalog();
    let n = catalog.len();
    if n == 0 {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        sel.0 = (sel.0 + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        sel.0 = (sel.0 + 1) % n;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let e = &catalog[sel.0.min(n - 1)];
        // unlock() spends the gold only on a new, affordable purchase.
        if meta.unlock(e.id, e.cost) {
            save_meta(&meta);
        }
    }
}
