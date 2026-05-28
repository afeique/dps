//! The egui **BUILD** screen (Phase UI): a tabbed pre-run configurator that
//! consolidates the armory / skills / loadout into one styled immediate-mode UI.
//! Opened with `B` from the title (`GameState::Build`); `Esc` returns. This is
//! the first egui surface in dps — the native overlays (armory/skills/loadout)
//! stay as-is for now; tabs here are fleshed out increment by increment.

use crate::meta::{save_meta, Meta, SP_STATS, SP_STAT_MAX_POINTS};
use crate::states::GameState;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

/// Which BUILD tab is showing.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Armory,
    Skills,
    Loadout,
}

/// The active BUILD tab (defaults to Skills).
#[derive(Resource)]
pub struct BuildTab(pub Tab);

impl Default for BuildTab {
    fn default() -> Self {
        Self(Tab::Skills)
    }
}

/// `B` on the title opens the BUILD screen.
pub fn open_build(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::KeyB) {
        next.set(GameState::Build);
    }
}

/// `Esc` closes the BUILD screen back to the title.
pub fn build_input(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Title);
    }
}

/// Draw the BUILD screen (runs in `EguiPrimaryContextPass` while in `Build`).
pub fn build_screen_ui(
    mut contexts: EguiContexts,
    mut meta: ResMut<Meta>,
    mut tab: ResMut<BuildTab>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(8.0);
        ui.heading(egui::RichText::new("◆ BUILD ◆").color(egui::Color32::from_rgb(120, 220, 255)));
        ui.label(
            egui::RichText::new(format!(
                "ACCOUNT GOLD {}     UNSPENT SP {}",
                meta.account_gold, meta.sp
            ))
            .color(egui::Color32::from_rgb(255, 220, 120)),
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.selectable_value(&mut tab.0, Tab::Armory, "ARMORY");
            ui.selectable_value(&mut tab.0, Tab::Skills, "SKILLS");
            ui.selectable_value(&mut tab.0, Tab::Loadout, "LOADOUT");
        });
        ui.separator();

        match tab.0 {
            Tab::Skills => {
                let mut spent = false;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for stat in SP_STATS.iter() {
                        let pts = meta.sp_points(stat.id);
                        let val = meta.sp_value(stat.id);
                        ui.horizontal(|ui| {
                            ui.monospace(format!(
                                "{:<13} {pts:>2}/{SP_STAT_MAX_POINTS}   +{val:>5.0} {}",
                                stat.name, stat.suffix
                            ));
                            let can = meta.sp > 0 && pts < SP_STAT_MAX_POINTS;
                            if ui.add_enabled(can, egui::Button::new("＋")).clicked()
                                && meta.allocate_sp(stat.id)
                            {
                                spent = true;
                            }
                        });
                    }
                });
                if spent {
                    save_meta(&meta);
                }
            }
            Tab::Armory => {
                ui.label("Armory unlocks — use the native A screen for now (egui tab next).");
            }
            Tab::Loadout => {
                ui.label("Ability loadout — use the native L screen for now (egui tab next).");
            }
        }

        ui.separator();
        ui.label(egui::RichText::new("Esc — back to title").weak());
    });
    Ok(())
}
