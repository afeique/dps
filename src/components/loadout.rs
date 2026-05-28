//! The 4-slot ability loadout (Phase AB). Replaces the fixed keybind skills with
//! a chosen loadout of up to four abilities triggered by keys, each with its own
//! cooldown. Ported from `combat/weapon-data.js` (`ABILITIES`) + `player.js:180`
//! slot state. This increment lands the **data model + cooldown bookkeeping** and
//! wires the abilities that already have effect code in dps; the remaining six
//! ability effects (drop-zone fields, Gravity Snare, Designator, …) and the BUILD-
//! tree picker land in follow-up increments.

use bevy::prelude::*;

/// The 14 selectable abilities (`combat/weapon-data.js` `ABILITIES`). Each maps to
/// an effect fired from a loadout slot. The five already implemented in dps
/// (Bulwark/FieldMedic/DeflectorOrbs/EmpPulse/Blink) dispatch live; the rest are
/// modeled here and gain effects in later increments (see [`Ability::implemented`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ability {
    /// 4 s window of 50% incoming-damage resist.
    Bulwark,
    /// Regen 3 HP/s for 5 s.
    FieldMedic,
    /// 3 orbs orbit the ship, each absorbing 3 enemy bullets.
    DeflectorOrbs,
    /// Stun every enemy in a radius for 2 s.
    EmpPulse,
    /// A turret that auto-fires at nearby enemies (deferred).
    SentryDrone,
    /// Teleport 220 px forward with a 0.35 s i-frame.
    Blink,
    /// Pull every enemy in r320 toward the ship (deferred).
    GravitySnare,
    /// Mark all enemies in r360 for 6 s (deferred).
    Designator,
    /// One-time death save (deferred).
    SecondWind,
    /// Override the player's offense element for 8 s (deferred).
    ElementalInfusion,
    /// Drop a Cryo field that chills enemies inside it (deferred).
    CryoField,
    /// Drop a Stasis field that slows enemies inside it (deferred).
    StasisField,
    /// Drop a Storm cell that shocks enemies inside it (deferred).
    StormCell,
    /// Drop a Pyre aura that burns enemies inside it (deferred).
    PyreAura,
}

impl Ability {
    /// Every ability, in catalog order (for the BUILD-tree picker + tests).
    pub const ALL: [Ability; 14] = [
        Ability::Bulwark,
        Ability::FieldMedic,
        Ability::DeflectorOrbs,
        Ability::EmpPulse,
        Ability::SentryDrone,
        Ability::Blink,
        Ability::GravitySnare,
        Ability::Designator,
        Ability::SecondWind,
        Ability::ElementalInfusion,
        Ability::CryoField,
        Ability::StasisField,
        Ability::StormCell,
        Ability::PyreAura,
    ];

    /// Cooldown in seconds (mirrors the legacy keybind cooldowns where they exist).
    pub fn cooldown(self) -> f32 {
        match self {
            Ability::Bulwark => 20.0,
            Ability::FieldMedic => 25.0,
            Ability::DeflectorOrbs => 15.0,
            Ability::EmpPulse => 22.0,
            Ability::SentryDrone => 18.0,
            Ability::Blink => 6.0,
            Ability::GravitySnare => 14.0,
            Ability::Designator => 16.0,
            Ability::SecondWind => 90.0,
            Ability::ElementalInfusion => 16.0,
            // Drop-zone fields (weapon-data.js): Cryo 18 s, the rest 16 s.
            Ability::CryoField => 18.0,
            Ability::StasisField | Ability::StormCell | Ability::PyreAura => 16.0,
        }
    }

    /// Active-effect duration in seconds (0 for instant abilities).
    pub fn duration(self) -> f32 {
        match self {
            Ability::Bulwark => 4.0,
            Ability::FieldMedic => 5.0,
            Ability::DeflectorOrbs => 5.0,
            Ability::SentryDrone => 8.0,
            Ability::Designator => 6.0,
            Ability::ElementalInfusion => 8.0,
            // All four drop-zone fields persist 5 s (weapon-data.js `duration`).
            Ability::CryoField
            | Ability::StasisField
            | Ability::StormCell
            | Ability::PyreAura => 5.0,
            // EmpPulse / Blink / GravitySnare / SecondWind are instantaneous.
            _ => 0.0,
        }
    }

    /// Stable armory unlock id (namespaced `ABL_` so it can't collide with
    /// weapon / attunement ids in `Meta.unlocked`).
    pub fn id(self) -> &'static str {
        match self {
            Ability::Bulwark => "ABL_BULWARK",
            Ability::FieldMedic => "ABL_FIELD_MEDIC",
            Ability::DeflectorOrbs => "ABL_DEFLECTOR_ORBS",
            Ability::EmpPulse => "ABL_EMP_PULSE",
            Ability::SentryDrone => "ABL_SENTRY_DRONE",
            Ability::Blink => "ABL_BLINK",
            Ability::GravitySnare => "ABL_GRAVITY_SNARE",
            Ability::Designator => "ABL_DESIGNATOR",
            Ability::SecondWind => "ABL_SECOND_WIND",
            Ability::ElementalInfusion => "ABL_ELEMENTAL_INFUSION",
            Ability::CryoField => "ABL_CRYO_FIELD",
            Ability::StasisField => "ABL_STASIS_FIELD",
            Ability::StormCell => "ABL_STORM_CELL",
            Ability::PyreAura => "ABL_PYRE_AURA",
        }
    }

    /// Parse a stable [`Ability::id`] back into the variant (save data).
    pub fn from_id(s: &str) -> Option<Ability> {
        Ability::ALL.into_iter().find(|a| a.id() == s)
    }

    /// The four abilities of the default loadout are free; the other ten are
    /// armory-unlocked (mirrors the weapon base/exotic split).
    pub fn base_unlocked(self) -> bool {
        matches!(
            self,
            Ability::Bulwark | Ability::FieldMedic | Ability::DeflectorOrbs | Ability::EmpPulse
        )
    }

    /// Whether this ability is available to equip — a free base ability or one
    /// unlocked in the armory.
    pub fn is_available(self, meta: &crate::meta::Meta) -> bool {
        self.base_unlocked() || meta.is_unlocked(self.id())
    }
}

/// The loadout-picker slot options: "empty" (`None`) plus every *available*
/// ability (base + armory-unlocked), in catalog order.
pub fn available_abilities(meta: &crate::meta::Meta) -> Vec<Option<Ability>> {
    let mut opts = vec![None];
    opts.extend(
        Ability::ALL
            .iter()
            .copied()
            .filter(|a| a.is_available(meta))
            .map(Some),
    );
    opts
}

/// Step a slot's ability to the next/prev entry in [`available_abilities`]
/// (wrapping). Pure — drives the loadout picker's Left/Right.
pub fn cycle_slot_ability(
    current: Option<Ability>,
    meta: &crate::meta::Meta,
    forward: bool,
) -> Option<Ability> {
    let opts = available_abilities(meta);
    let i = opts.iter().position(|&o| o == current).unwrap_or(0);
    let n = opts.len();
    let j = if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    };
    opts[j]
}

impl Ability {

    /// A 3-char tag for the HUD ability-bar slot.
    pub fn short(self) -> &'static str {
        match self {
            Ability::Bulwark => "BLW",
            Ability::FieldMedic => "MED",
            Ability::DeflectorOrbs => "ORB",
            Ability::EmpPulse => "EMP",
            Ability::SentryDrone => "DRN",
            Ability::Blink => "BLK",
            Ability::GravitySnare => "GRV",
            Ability::Designator => "MRK",
            Ability::SecondWind => "2ND",
            Ability::ElementalInfusion => "INF",
            Ability::CryoField => "CRY",
            Ability::StasisField => "STA",
            Ability::StormCell => "STM",
            Ability::PyreAura => "PYR",
        }
    }

    /// Display name (HUD ability bar + BUILD tree).
    pub fn name(self) -> &'static str {
        match self {
            Ability::Bulwark => "Bulwark",
            Ability::FieldMedic => "Field Medic",
            Ability::DeflectorOrbs => "Deflector Orbs",
            Ability::EmpPulse => "EMP Pulse",
            Ability::SentryDrone => "Sentry Drone",
            Ability::Blink => "Blink",
            Ability::GravitySnare => "Gravity Snare",
            Ability::Designator => "Designator",
            Ability::SecondWind => "Second Wind",
            Ability::ElementalInfusion => "Elemental Infusion",
            Ability::CryoField => "Cryo Field",
            Ability::StasisField => "Stasis Field",
            Ability::StormCell => "Storm Cell",
            Ability::PyreAura => "Pyre Aura",
        }
    }

    /// Whether this ability's effect is wired into `activate_loadout`. All 14 are
    /// now implemented; kept as a guard so a new enum variant defaults to no-op
    /// until wired.
    pub fn implemented(self) -> bool {
        matches!(
            self,
            Ability::Bulwark
                | Ability::FieldMedic
                | Ability::DeflectorOrbs
                | Ability::EmpPulse
                | Ability::SentryDrone
                | Ability::Blink
                | Ability::GravitySnare
                | Ability::Designator
                | Ability::SecondWind
                | Ability::ElementalInfusion
                | Ability::CryoField
                | Ability::StasisField
                | Ability::StormCell
                | Ability::PyreAura
        )
    }

    /// For a drop-zone field ability, its `(status, radius, tick interval)`
    /// (`weapon-data.js`). `None` for non-field abilities. Duration is
    /// [`Ability::duration`] (5 s for every field).
    pub fn field_params(self) -> Option<(FieldStatus, f32, f32)> {
        match self {
            Ability::CryoField => Some((FieldStatus::Freeze, 180.0, 0.25)),
            Ability::StasisField => Some((FieldStatus::Chill, 210.0, 0.25)),
            Ability::StormCell => Some((FieldStatus::Conduct, 200.0, 0.30)),
            Ability::PyreAura => Some((FieldStatus::Burn, 190.0, 0.40)),
            _ => None,
        }
    }
}

/// The status a [`AbilityField`] re-applies to enemies inside it each tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldStatus {
    /// Cryo Field — halts enemies (`Frozen`).
    Freeze,
    /// Stasis Field — slows enemies (`Chill`).
    Chill,
    /// Storm Cell — makes enemies conductive / shocked (`Conduct`).
    Conduct,
    /// Pyre Aura — burns enemies (`Burning`, 6 dps).
    Burn,
}

/// A dropped drop-zone field that re-applies [`FieldStatus`] to every enemy
/// within `radius` every `tick` seconds, until `secs` elapses. Spawned by the
/// Cryo/Stasis/Storm/Pyre abilities and ticked by
/// `systems::abilities::tick_ability_fields`.
#[derive(Component, Debug, Clone, Copy)]
pub struct AbilityField {
    pub status: FieldStatus,
    pub radius: f32,
    /// Remaining lifetime (s).
    pub secs: f32,
    /// Interval between status applications (s).
    pub tick: f32,
    /// Countdown to the next application (s).
    pub timer: f32,
}

/// A **Sentry Drone** (AB ability): orbits the player and auto-fires at the
/// nearest enemy until `secs` elapses (`player/abilities.js`). Ticked by
/// `systems::abilities::tick_sentry_drones`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Sentry {
    /// Remaining lifetime (s).
    pub secs: f32,
    /// Current orbit angle (rad).
    pub angle: f32,
    /// Countdown to the next shot (s).
    pub fire_timer: f32,
}

/// Armed by the **Second Wind** ability — a one-time death save. When the player
/// would take a lethal hit while this marker is present, `damage::apply_damage`
/// consumes it instead of ending the run: full-HP revive + a spare tank
/// (`lifecycle.js`). Re-armed by casting the ability again (after its cooldown).
#[derive(Component, Debug, Clone, Copy)]
pub struct SecondWindArmed;

/// The player's equipped abilities — slot `i` is fired by ability key `i`. `None`
/// is an empty slot. Defaults to the four abilities dps already implements.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct EquippedAbilities(pub [Option<Ability>; 4]);

impl Default for EquippedAbilities {
    fn default() -> Self {
        Self([
            Some(Ability::Bulwark),
            Some(Ability::FieldMedic),
            Some(Ability::DeflectorOrbs),
            Some(Ability::EmpPulse),
        ])
    }
}

/// Per-slot cooldown bookkeeping, parallel to [`EquippedAbilities`]. `remaining`
/// counts down to 0 (ready); `max` is the last-triggered cooldown, used for the
/// HUD's bottom-up fill.
#[derive(Resource, Debug, Default, Clone)]
pub struct AbilityCooldowns {
    pub remaining: [f32; 4],
    pub max: [f32; 4],
}

impl AbilityCooldowns {
    /// Tick every slot's cooldown toward ready.
    pub fn tick(&mut self, dt: f32) {
        for r in &mut self.remaining {
            *r = (*r - dt).max(0.0);
        }
    }

    /// A slot is ready to fire when its cooldown has elapsed.
    pub fn is_ready(&self, slot: usize) -> bool {
        slot < 4 && self.remaining[slot] <= 0.0
    }

    /// Put a slot on cooldown (called when its ability fires).
    pub fn trigger(&mut self, slot: usize, cooldown: f32) {
        if slot < 4 {
            self.remaining[slot] = cooldown;
            self.max[slot] = cooldown;
        }
    }

    /// Fraction of the cooldown still remaining in `0..=1` (HUD fill). 0 when ready.
    pub fn fraction(&self, slot: usize) -> f32 {
        if slot < 4 && self.max[slot] > 0.0 {
            (self.remaining[slot] / self.max[slot]).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}
