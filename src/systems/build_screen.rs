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
use crate::render::shapes::SKINS;
use crate::systems::cores::{
    reroll_cost, reroll_stash_item, salvage_value, tier_up_cost, tier_up_stash_item,
};
use crate::systems::difficulty::DIFFICULTIES;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

/// Which BUILD tab is showing.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Stats,
    Armory,
    Skills,
    Loadout,
    Stash,
    Skins,
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

/// A cohesive dark-space theme for the BUILD screen: deep-navy panels, soft
/// cyan-white text, and cyan selection/links to match the game's HDR palette.
/// Only touches widely-stable `Visuals` fields, set fresh each frame.
fn apply_build_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = egui::Color32::from_rgb(8, 12, 20); // deep space navy
    v.override_text_color = Some(egui::Color32::from_rgb(200, 225, 240));
    v.selection.bg_fill = egui::Color32::from_rgb(20, 72, 98); // cyan-tinted highlight
    v.hyperlink_color = egui::Color32::from_rgb(120, 220, 255);
    ctx.set_visuals(v);
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
    apply_build_theme(ctx);
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
            ui.selectable_value(&mut tab.0, Tab::Stats, "STATS");
            ui.selectable_value(&mut tab.0, Tab::Armory, "ARMORY");
            ui.selectable_value(&mut tab.0, Tab::Skills, "SKILLS");
            ui.selectable_value(&mut tab.0, Tab::Loadout, "LOADOUT");
            ui.selectable_value(&mut tab.0, Tab::Stash, "STASH");
            ui.selectable_value(&mut tab.0, Tab::Skins, "SHIP");
        });
        ui.separator();

        match tab.0 {
            Tab::Skins => {
                ui.label("Ship skin (cosmetic — recolours the hull's glow edge):");
                let current = meta.skin;
                let mut pick: Option<usize> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, skin) in SKINS.iter().enumerate() {
                        ui.horizontal(|ui| {
                            // HDR edge → a hue-preserving swatch (brightest channel = 255).
                            let (r, g, b) = skin.edge;
                            let m = r.max(g).max(b).max(1.0);
                            let swatch = egui::Color32::from_rgb(
                                (r / m * 255.0) as u8,
                                (g / m * 255.0) as u8,
                                (b / m * 255.0) as u8,
                            );
                            ui.colored_label(swatch, "◆");
                            ui.monospace(format!("{:<10}", skin.name));
                            if i == current {
                                ui.colored_label(
                                    egui::Color32::from_rgb(120, 230, 140),
                                    "EQUIPPED",
                                );
                            } else if ui.button("Select").clicked() {
                                pick = Some(i);
                            }
                        });
                    }
                });
                if let Some(i) = pick {
                    meta.skin = i;
                    save_meta(&meta);
                }
            }
            Tab::Stats => {
                // Run difficulty selector (Phase X) — scales the run's enemy HP.
                ui.horizontal(|ui| {
                    ui.monospace("Difficulty:");
                    for (i, (name, hp, reward)) in DIFFICULTIES.iter().enumerate() {
                        let i = i as u8;
                        let label = format!("{name} (×{hp} HP, ×{reward} gold)");
                        if ui.selectable_label(meta.difficulty == i, label).clicked() {
                            meta.difficulty = i;
                            save_meta(&meta);
                        }
                    }
                });
                ui.separator();
                // Read-only account overview (backed by Meta::account_summary).
                egui::Grid::new("stats_grid")
                    .num_columns(2)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        for (label, value) in meta.account_summary() {
                            ui.monospace(label);
                            ui.monospace(
                                egui::RichText::new(value)
                                    .color(egui::Color32::from_rgb(120, 220, 255)),
                            );
                            ui.end_row();
                        }
                    });
            }
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
