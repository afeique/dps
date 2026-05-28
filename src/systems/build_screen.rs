//! The egui **BUILD** screen (Phase UI): a tabbed pre-run configurator that
//! consolidates the armory / skills / loadout into one styled immediate-mode UI.
//! Opened with `B` from the title (`GameState::Build`); `Esc` returns. This is
//! the first egui surface in dps — the native overlays (armory/skills/loadout)
//! stay as-is for now; tabs here are fleshed out increment by increment.

use crate::components::loadout::{cycle_slot_ability, EquippedAbilities};
use crate::meta::{save_meta, Meta, SP_STATS, SP_STAT_MAX_POINTS};
use crate::resources::GameRng;
use crate::states::GameState;
use crate::systems::armory::armory_catalog;
use crate::systems::cores::{
    reroll_cost, reroll_stash_item, salvage_value, tier_up_cost, tier_up_stash_item,
};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

/// Which BUILD tab is showing.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Armory,
    Skills,
    Loadout,
    Stash,
}

/// A stash action queued by a button click this frame, applied after the list is
/// drawn (so we don't mutate `meta.stash` while iterating it).
enum StashAction {
    Salvage(usize),
    Reroll(usize),
    TierUp(usize),
    SalvageAll,
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
    mut equipped: ResMut<EquippedAbilities>,
    mut rng: ResMut<GameRng>,
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
            ui.selectable_value(&mut tab.0, Tab::Stash, "STASH");
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
                let catalog = armory_catalog();
                let mut bought = false;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &catalog {
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{:<20}", entry.name));
                            if meta.is_unlocked(entry.id) {
                                ui.colored_label(
                                    egui::Color32::from_rgb(120, 230, 140),
                                    "UNLOCKED",
                                );
                            } else {
                                let afford = meta.account_gold >= entry.cost;
                                let btn = egui::Button::new(format!("{} g", entry.cost));
                                if ui.add_enabled(afford, btn).clicked()
                                    && meta.unlock(entry.id, entry.cost)
                                {
                                    bought = true;
                                }
                            }
                        });
                    }
                });
                if bought {
                    save_meta(&meta);
                }
            }
            Tab::Loadout => {
                let mut changed = false;
                ui.label("4 ability slots (◀ ▶ cycle through unlocked abilities):");
                for slot in 0..4usize {
                    ui.horizontal(|ui| {
                        let name = equipped.0[slot].map_or("— empty —", |a| a.name());
                        ui.monospace(format!("Slot {}:  {:<18}", slot + 1, name));
                        if ui.button("◀").clicked() {
                            equipped.0[slot] = cycle_slot_ability(equipped.0[slot], &meta, false);
                            changed = true;
                        }
                        if ui.button("▶").clicked() {
                            equipped.0[slot] = cycle_slot_ability(equipped.0[slot], &meta, true);
                            changed = true;
                        }
                    });
                }
                if changed {
                    meta.set_ability_loadout(&equipped.0);
                    save_meta(&meta);
                }
            }
            Tab::Stash => {
                ui.label(
                    egui::RichText::new(format!("CORES {}     STASH {}", meta.cores, meta.stash.len()))
                        .color(egui::Color32::from_rgb(180, 240, 255)),
                );
                let mut action: Option<StashAction> = None;
                if !meta.stash.is_empty() && ui.button("Salvage All").clicked() {
                    action = Some(StashAction::SalvageAll);
                }
                ui.separator();
                if meta.stash.is_empty() {
                    ui.weak("Empty — unequipped drops bank here to salvage or upgrade.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Read-only row render; clicks only set `action` (a local), so
                    // `meta` is never mutated mid-iteration.
                    for i in 0..meta.stash.len() {
                        let item = &meta.stash[i];
                        let (rcost, tcost, sval) =
                            (reroll_cost(item), tier_up_cost(item), salvage_value(item));
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{:<22} L{:<3}", item.name, item.level));
                            if ui.button(format!("Salvage +{sval}")).clicked() {
                                action = Some(StashAction::Salvage(i));
                            }
                            let can_reroll = meta.cores >= rcost;
                            if ui
                                .add_enabled(can_reroll, egui::Button::new(format!("Reroll {rcost}")))
                                .clicked()
                            {
                                action = Some(StashAction::Reroll(i));
                            }
                            match tcost {
                                Some(c) => {
                                    if ui
                                        .add_enabled(
                                            meta.cores >= c,
                                            egui::Button::new(format!("Tier-Up {c}")),
                                        )
                                        .clicked()
                                    {
                                        action = Some(StashAction::TierUp(i));
                                    }
                                }
                                None => {
                                    ui.weak("MAX");
                                }
                            }
                        });
                    }
                });
                if let Some(act) = action {
                    match act {
                        StashAction::Salvage(i) => {
                            meta.salvage_item(i);
                        }
                        StashAction::SalvageAll => {
                            meta.salvage_all();
                        }
                        StashAction::Reroll(i) => {
                            reroll_stash_item(&mut meta, &mut rng, i);
                        }
                        StashAction::TierUp(i) => {
                            tier_up_stash_item(&mut meta, &mut rng, i);
                        }
                    }
                    save_meta(&meta);
                }
            }
        }

        ui.separator();
        ui.label(egui::RichText::new("Esc — back to title").weak());
    });
    Ok(())
}
