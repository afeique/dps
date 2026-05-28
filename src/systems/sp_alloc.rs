//! The SKILL-POINTS screen (Phase ME): spend banked account SP across the 12
//! permanent stats ([`crate::meta::SP_STATS`]). Reached with `S` from the title
//! (its own `GameState::SpAllocation`); `Esc` returns. A native-UI overlay
//! mirroring the armory. Allocations persist via [`save_meta`]; the run reads the
//! effective bonuses via `Meta::sp_value` (effect-wiring lands next).

use crate::meta::{save_meta, Meta, SP_STATS, SP_STAT_MAX_POINTS};
use crate::states::GameState;
use bevy::prelude::*;

/// Cursor over the SP-stat list.
#[derive(Resource, Default)]
pub struct SpAllocSel(pub usize);

/// Root of the SP overlay (despawned on exit via `flow::despawn_screen`).
#[derive(Component)]
pub struct SpPanel;

/// The live SP text (rebuilt each frame).
#[derive(Component)]
pub struct SpText;

/// `S` on the title screen opens the skill-points screen (resetting the cursor).
pub fn open_sp_alloc(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<SpAllocSel>,
) {
    if keys.just_pressed(KeyCode::KeyS) {
        sel.0 = 0;
        next.set(GameState::SpAllocation);
    }
}

pub fn spawn_sp_ui(mut commands: Commands) {
    commands
        .spawn((
            SpPanel,
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
            BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.92)),
        ))
        .with_children(|p| {
            p.spawn((
                SpText,
                Text::new(""),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.95, 1.0)),
            ));
        });
}

/// Rebuild the SP text from banked SP + per-stat allocation + cursor.
pub fn sp_ui_update(meta: Res<Meta>, sel: Res<SpAllocSel>, mut q: Query<&mut Text, With<SpText>>) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let mut s = format!("=== SKILL POINTS ===        UNSPENT SP: {}\n\n", meta.sp);
    for (i, stat) in SP_STATS.iter().enumerate() {
        let cursor = if i == sel.0 { ">" } else { " " };
        let pts = meta.sp_points(stat.id);
        let val = meta.sp_value(stat.id);
        // Whole-number stats (HP, energy) read cleaner without a decimal.
        let val_str = if (val - val.round()).abs() < 1e-3 {
            format!("{}", val.round() as i64)
        } else {
            format!("{val:.1}")
        };
        s.push_str(&format!(
            "{cursor} {:<13} {pts:>2}/{SP_STAT_MAX_POINTS}   +{val_str} {}\n",
            stat.name, stat.suffix
        ));
    }
    s.push_str("\n[ Up/Down select   ENTER spend SP   ESC back ]");
    if text.0 != s {
        text.0 = s;
    }
}

/// Navigate (Up/Down arrows), spend 1 SP on the selected stat (ENTER), back
/// (Esc). Nav is arrows-only here — `W/S` are skipped since `S` opens this screen.
pub fn sp_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    mut sel: ResMut<SpAllocSel>,
    mut meta: ResMut<Meta>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Title);
        return;
    }
    let n = SP_STATS.len();
    if keys.just_pressed(KeyCode::ArrowUp) {
        sel.0 = (sel.0 + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        sel.0 = (sel.0 + 1) % n;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let id = SP_STATS[sel.0.min(n - 1)].id;
        if meta.allocate_sp(id) {
            save_meta(&meta);
        }
    }
}
