//! Element & resistance taxonomy + multiplier math — ported 1:1 from
//! `js/modules/combat/elements.js` (rainboids v6.161, "Phase E1").
//!
//! **E1 is foundation-only.** This establishes the 7-element taxonomy, each
//! element's signature on-hit status id, the resist/weakness multiplier helpers,
//! the multi-element (attunement) average, and the adaptive-resist (Warden)
//! helpers. NOTHING reads it yet — adding this module changes no gameplay until
//! later increments wire it into the damage pipeline (E2 player→enemy, E5
//! enemy→player), the status engine (E3), the reaction verbs (E4), and weapon
//! attunements (W1). See `docs/roguelite-port-plan.md` Phase E.

use bevy::prelude::*;

/// The seven damage elements (`ELEMENTS`, elements.js:15-23). `Kinetic` is the
/// neutral physical baseline (no signature status); the other six each apply a
/// signature on-hit status when the status engine (E3) lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Kinetic,
    Pyro,
    Cryo,
    Volt,
    Toxic,
    Void,
    Radiant,
}

/// Number of elements (sizes the [`Resistances`] array).
pub const ELEMENT_COUNT: usize = 7;

impl Element {
    /// All seven, in `ELEMENT_IDS` order (also the [`Resistances`] index order).
    pub const ALL: [Element; ELEMENT_COUNT] = [
        Element::Kinetic,
        Element::Pyro,
        Element::Cryo,
        Element::Volt,
        Element::Toxic,
        Element::Void,
        Element::Radiant,
    ];

    /// Index into a [`Resistances`] array.
    pub const fn idx(self) -> usize {
        match self {
            Element::Kinetic => 0,
            Element::Pyro => 1,
            Element::Cryo => 2,
            Element::Volt => 3,
            Element::Toxic => 4,
            Element::Void => 5,
            Element::Radiant => 6,
        }
    }

    /// Stable string id (matches the JS `ELEMENTS[k].id`) — for save data, data
    /// tables, and event-name lookups.
    pub fn id(self) -> &'static str {
        match self {
            Element::Kinetic => "KINETIC",
            Element::Pyro => "PYRO",
            Element::Cryo => "CRYO",
            Element::Volt => "VOLT",
            Element::Toxic => "TOXIC",
            Element::Void => "VOID",
            Element::Radiant => "RADIANT",
        }
    }

    /// Parse a JS element id back to an `Element` (data tables / save data).
    pub fn from_id(s: &str) -> Option<Element> {
        Some(match s {
            "KINETIC" => Element::Kinetic,
            "PYRO" => Element::Pyro,
            "CRYO" => Element::Cryo,
            "VOLT" => Element::Volt,
            "TOXIC" => Element::Toxic,
            "VOID" => Element::Void,
            "RADIANT" => Element::Radiant,
            _ => return None,
        })
    }

    /// Display name (`ELEMENTS[k].name`).
    pub fn name(self) -> &'static str {
        match self {
            Element::Kinetic => "Kinetic",
            Element::Pyro => "Pyro",
            Element::Cryo => "Cryo",
            Element::Volt => "Volt",
            Element::Toxic => "Toxic",
            Element::Void => "Void",
            Element::Radiant => "Radiant",
        }
    }

    /// Identity HDR color (`ELEMENTS[k].color`) — bright so element-tinted
    /// bullets/beams feed the bloom.
    pub fn color(self) -> Color {
        match self {
            Element::Kinetic => Color::srgb_u8(0xdf, 0xe7, 0xf0),
            Element::Pyro => Color::srgb_u8(0xff, 0x55, 0x22),
            Element::Cryo => Color::srgb_u8(0x66, 0xcc, 0xff),
            Element::Volt => Color::srgb_u8(0xa8, 0x55, 0xff),
            Element::Toxic => Color::srgb_u8(0x88, 0xff, 0x44),
            Element::Void => Color::srgb_u8(0x77, 0x44, 0xdd),
            Element::Radiant => Color::srgb_u8(0xff, 0xee, 0x88),
        }
    }

    /// The signature status this element applies on hit (`statusId`); `Kinetic`
    /// has none. Returned as the JS status id for now — the concrete status
    /// *effects* land in the E3 status increment (this is just the taxonomy
    /// link). `BRN`/`CHILL`/`CONDUCT`/`CORRODE`/`MARK`/`PURGE`.
    pub fn status_id(self) -> Option<&'static str> {
        match self {
            Element::Kinetic => None,
            Element::Pyro => Some("BRN"),
            Element::Cryo => Some("CHILL"),
            Element::Volt => Some("CONDUCT"),
            Element::Toxic => Some("CORRODE"),
            Element::Void => Some("MARK"),
            Element::Radiant => Some("PURGE"),
        }
    }
}

/// Damage multiplier for a single resist value `r` (`elementalMultiplier`,
/// elements.js:39-44):
///
/// - `r > 0` → resistant (multiplier `< 1`)
/// - `r = 1` → immune (multiplier `0`)
/// - `r < 0` → weak (multiplier `> 1` — a damage bonus)
/// - `r = 0` → neutral (multiplier `1`)
///
/// Clamped to `[0, 2]` so a single weakness can't exceed +100%.
pub fn elemental_multiplier(resist: f32) -> f32 {
    (1.0 - resist).clamp(0.0, 2.0)
}

/// Resolve the active element list for a shot (`resolveBulletElements`,
/// elements.js:72-80), in priority order:
///
/// 1. an `override_el` (Elemental Infusion / Overdrive) — replaces all
/// 2. the equipped attunement elements (deduped, order-preserved) — the stack
/// 3. the weapon's `base` element
///
/// Always returns a non-empty list.
pub fn resolve_bullet_elements(
    override_el: Option<Element>,
    attunements: &[Element],
    base: Element,
) -> Vec<Element> {
    if let Some(o) = override_el {
        return vec![o];
    }
    let mut seen: Vec<Element> = Vec::with_capacity(attunements.len());
    for &e in attunements {
        if !seen.contains(&e) {
            seen.push(e);
        }
    }
    if !seen.is_empty() {
        return seen;
    }
    vec![base]
}

/// A small set of elements carried by a shot (a bitset over [`Element`]). Cheap
/// + `Copy` so it rides on every bullet without a heap allocation. Empty =
/// neutral (multiplier 1). Built from the resolved attunement/base elements
/// (W1); the per-hit resist average reads it via [`Resistances::multi_multiplier_set`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ElementSet(u8);

impl ElementSet {
    /// The empty (neutral) set.
    pub const EMPTY: ElementSet = ElementSet(0);

    /// A single-element set.
    pub fn single(e: Element) -> Self {
        ElementSet(1 << e.idx())
    }

    /// A `Kinetic`-only set — the default for an un-attuned shot.
    pub fn kinetic() -> Self {
        Self::single(Element::Kinetic)
    }

    /// Build a set from a slice (deduping naturally).
    pub fn from_slice(els: &[Element]) -> Self {
        let mut bits = 0u8;
        for &e in els {
            bits |= 1 << e.idx();
        }
        ElementSet(bits)
    }

    /// Is `e` present?
    pub fn contains(self, e: Element) -> bool {
        self.0 & (1 << e.idx()) != 0
    }

    /// Add `e` to the set.
    pub fn insert(&mut self, e: Element) {
        self.0 |= 1 << e.idx();
    }

    /// Number of elements present.
    pub fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// True when no element is present (neutral).
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Iterate the present elements in `Element::ALL` order.
    pub fn iter(self) -> impl Iterator<Item = Element> {
        Element::ALL.into_iter().filter(move |&e| self.contains(e))
    }
}

/// Per-target elemental resistances (the `resist` map on enemies + the player's
/// item resist affixes). One slot per [`Element`]: `>0` resistant, `<0` weak,
/// `1` immune, `0` neutral. Default = all-neutral.
///
/// For Warden-type adaptive enemies the same map is mutated by [`Resistances::adapt`]
/// (after a hit) and [`Resistances::decay`] (on a slow timer) — their base map
/// starts neutral so adaptation never corrupts a fixed profile.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Resistances {
    r: [f32; ELEMENT_COUNT],
}

impl Resistances {
    /// All-neutral resistances.
    pub fn new() -> Self {
        Self::default()
    }

    /// This target's resist value for `e` (`0` = neutral).
    pub fn get(&self, e: Element) -> f32 {
        self.r[e.idx()]
    }

    /// Set `e`'s resist in place.
    pub fn set(&mut self, e: Element, v: f32) {
        self.r[e.idx()] = v;
    }

    /// Builder form — set one element's resist and return self (for the static
    /// enemy resist tables, e.g. `Resistances::new().with(Pyro, 0.6).with(Cryo, -0.4)`).
    pub fn with(mut self, e: Element, v: f32) -> Self {
        self.r[e.idx()] = v;
        self
    }

    /// Single-element damage multiplier (`elementalMultiplier(resist, element)`).
    pub fn multiplier(&self, e: Element) -> f32 {
        elemental_multiplier(self.get(e))
    }

    /// Incoming-damage multiplier for the **player** vs element `e`:
    /// `1 − clamp(resist, 0, 0.9)` (`playerElementResistMult`, lifecycle.js).
    /// Gear can't grant full immunity (cap 0.9) or amplify (floor 0) — the
    /// symmetric counterpart to the enemy-side [`Resistances::multiplier`].
    pub fn player_multiplier(&self, e: Element) -> f32 {
        1.0 - self.get(e).clamp(0.0, 0.9)
    }

    /// Multi-element damage multiplier (`multiElementMultiplier`, elements.js:57-63):
    /// no elements → `1`; one element → its multiplier; several → the **average**
    /// of the per-element multipliers (the W1 attunement "focus vs coverage" lever).
    pub fn multi_multiplier(&self, elements: &[Element]) -> f32 {
        match elements.len() {
            0 => 1.0,
            1 => self.multiplier(elements[0]),
            n => elements.iter().map(|&e| self.multiplier(e)).sum::<f32>() / n as f32,
        }
    }

    /// [`Resistances::multi_multiplier`] for an [`ElementSet`] (the on-bullet
    /// form): empty set → `1`; else the AVERAGE of the present elements' multipliers.
    pub fn multi_multiplier_set(&self, set: ElementSet) -> f32 {
        let n = set.len();
        if n == 0 {
            return 1.0;
        }
        set.iter().map(|e| self.multiplier(e)).sum::<f32>() / n as f32
    }

    /// The element this target is most WEAK to at/below `threshold` (default
    /// −0.3 via [`Resistances::weakest`]), for the weakness-telegraph pip
    /// (`weaknessElement`). `None` when there's no notable weakness. Ties resolve
    /// to the first in `Element::ALL` order.
    pub fn weakness(&self, threshold: f32) -> Option<Element> {
        let mut worst = threshold;
        let mut el = None;
        for e in Element::ALL {
            let v = self.get(e);
            if v < worst {
                worst = v;
                el = Some(e);
            }
        }
        el
    }

    /// [`Resistances::weakness`] at the default −0.3 threshold.
    pub fn weakest(&self) -> Option<Element> {
        self.weakness(-0.3)
    }

    /// Adaptive-resist bump (Warden, `adaptResist`, elements.js:114-117): nudge
    /// `e`'s resist toward `cap` by `step` (defaults via [`Resistances::adapt_default`]).
    /// Call AFTER a hit lands so the CURRENT hit uses the old resist and the
    /// adaptation shows on the NEXT same-element hit.
    pub fn adapt(&mut self, e: Element, step: f32, cap: f32) {
        let i = e.idx();
        self.r[i] = (self.r[i] + step).min(cap);
    }

    /// [`Resistances::adapt`] with the JS defaults (step 0.12, cap 0.75).
    pub fn adapt_default(&mut self, e: Element) {
        self.adapt(e, 0.12, 0.75);
    }

    /// Adaptive-resist decay (`decayResistMap`, elements.js:126-133): scale each
    /// *adapted* (positive) entry toward 0 by `factor`; entries at/under `floor`
    /// snap to 0. Base weaknesses (negative) are left untouched. **Only call on
    /// adaptive enemies** — a fixed resist profile must not decay.
    pub fn decay(&mut self, factor: f32, floor: f32) {
        for v in &mut self.r {
            if *v > 0.0 {
                *v *= factor;
                if *v <= floor {
                    *v = 0.0;
                }
            }
        }
    }

    /// [`Resistances::decay`] with the JS defaults (factor 0.8, floor 0.02).
    pub fn decay_default(&mut self) {
        self.decay(0.8, 0.02);
    }
}
