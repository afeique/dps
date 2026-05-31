//! Player-ship components. Ported from `js/modules/player/*` and
//! `js/modules/combat/weapon-data.js`.

use bevy::prelude::*;

/// Spare **health tanks** (`healthTanks`, spec II.2). On a lethal hit, if
/// `count > 0` one tank is consumed and HP refills *in place* — no respawn
/// delay, no invulnerability (spec II.2) — instead of triggering `GameOver`.
/// Start = 1 (total effective lives = `count + 1`, the active bar being the
/// "+1"); JS `MAX_HEALTH_TANKS = 3`.
#[derive(Component, Debug)]
pub struct Lives {
    pub count: u32,
    /// Overheal credit toward the next tank (spec II.2): each full `1.0`
    /// (`TANK_OVERFLOW_HP = 40` overheal) grants +1 tank up to the cap.
    pub progress: f32,
}

/// Spare-tank cap (spec II.2 `MAX_HEALTH_TANKS`).
pub const MAX_TANKS: u32 = 3;
/// Overheal HP that equals one tank of progress (spec II.2 `TANK_OVERFLOW_HP`).
pub const TANK_OVERFLOW_HP: f32 = 40.0;

/// Base shield damage-reduction (spec II.2: 15%).
pub const BASE_SHIELD_REDUCTION: f32 = 0.15;
/// Shield damage-reduction cap (spec II.2: 75%).
pub const SHIELD_REDUCTION_CAP: f32 = 0.75;

/// Energy shield as a flat **damage-reduction fraction** (spec II.2) — *not* an
/// absorbing HP pool. Incoming damage to the player is scaled by
/// `(1 − reduction)` before it reaches `Health`. Base 15%, cap 75% with
/// upgrades.
#[derive(Component, Debug, Clone, Copy)]
pub struct Shield {
    /// Damage-reduction fraction, in `[0, SHIELD_REDUCTION_CAP]`.
    pub reduction: f32,
}

/// Active **Bulwark** skill window (spec III.4): while present, incoming player
/// damage is halved (after the shield, per the spec pipeline step 7). Counted
/// down + removed by `skills::tick_bulwark`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Bulwark {
    pub seconds: f32,
}

/// Bulwark damage-resist fraction (spec III.4: 50%, 65% with IRON_WILL).
pub const BULWARK_RESIST: f32 = 0.5;

/// Active **Repair Nanites** skill window (spec III.4): regenerate `rate` HP/s
/// for `seconds`. Counted down + applied by `skills::tick_repair`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Repairing {
    pub seconds: f32,
    pub rate: f32,
}

/// A **Deflector Orb** (spec III.4): orbits the ship and absorbs `blocks` enemy
/// bullets before popping. `phase` is its starting angle around the ship;
/// `skills::orbit_deflectors` positions it, `skills::deflector_blocks` resolves
/// hits. Despawned via `Lifetime` or when `blocks` hits 0.
#[derive(Component, Debug, Clone, Copy)]
pub struct DeflectorOrb {
    pub blocks: u32,
    pub phase: f32,
}

/// Deflector-orb orbit radius (spec III.4: r=45).
pub const DEFLECTOR_RADIUS: f32 = 45.0;

/// Active **Tractor Shield** skill window (spec III.4): for `seconds`, enemy
/// bullets inside a forward arc within range are absorbed into coins. Counted
/// down by `skills::tick_tractor`, consumed by `skills::tractor_absorb`.
#[derive(Component, Debug, Clone, Copy)]
pub struct TractorShield {
    pub seconds: f32,
}

/// Tractor capture range (spec III.4: bullets within 55 px).
pub const TRACTOR_RANGE: f32 = 55.0;
/// Tractor forward half-arc (spec III.4 base arc π/2 → half π/4).
pub const TRACTOR_HALF_ARC: f32 = std::f32::consts::FRAC_PI_4;
/// Coins minted per absorbed bullet (spec III.4: `5 + 5*PROFIT`; base 5).
pub const TRACTOR_COINS: u64 = 5;

// ── Player elemental statuses (Phase E5, from `player-status.js`) ─────────────
// Enemy attacks carry an element; on a hit the element's signature status lands
// on the ship: PYRO→burn DoT, CRYO→chill (slow), TOXIC→corrode (amplify incoming).
// VOLT/VOID/RADIANT apply no player status. Applied by
// `systems::player_status::apply_player_status`; ticked in `FixedUpdate`.

/// Burn DoT on the player (`PlayerBurn`, PYRO). Ticks a **chunk** every
/// `PLAYER_BURN_TICK_SECS` (not dt-scaled) — player damage is `.round()`ed, so a
/// per-frame fraction would vanish; the 500 ms chunk survives the round (like the
/// JS 500 ms burn tick). `tick` counts down to the next chunk; `secs` to expiry.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerBurn {
    pub secs: f32,
    pub tick: f32,
}

/// Chill on the player (`PlayerChill`, CRYO): slows the ship to `PLAYER_CHILL_SLOW`×.
/// (The movement-slow effect is wired in E5b; the component + lifetime land now.)
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerChill {
    pub secs: f32,
}

/// Corrode on the player (`PlayerCorrode`, TOXIC): incoming damage ×(1 +
/// `PLAYER_CORRODE_PER_STACK`×stacks), `stacks` capped at `PLAYER_CORRODE_MAX`.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerCorrode {
    pub stacks: u32,
    pub secs: f32,
}

/// Player-status tuning (`player-status.js`): burn 2/tick every 0.5 s for 3 s
/// (6 ticks = 12 total), chill 1.5 s, corrode cap 2 / 3 s.
pub const PLAYER_BURN_PER_TICK: f32 = 2.0;
pub const PLAYER_BURN_TICK_SECS: f32 = 0.5;
pub const PLAYER_BURN_SECS: f32 = 3.0;
pub const PLAYER_CHILL_SECS: f32 = 1.5;
pub const PLAYER_CORRODE_SECS: f32 = 3.0;
pub const PLAYER_CORRODE_MAX: u32 = 2;

/// Active **Overdrive** power-weapon buff (W, weapon-data.js): while present the
/// primary fires faster (cooldown ×[`OVERDRIVE_FIRE_MULT`]) and harder (damage
/// ×[`OVERDRIVE_DMG_MULT`]). Counted down by `power_weapon::tick_overdrive`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Overdrive {
    pub secs: f32,
}

/// Overdrive tuning (weapon-data.js): 4.5 s, ×0.55 fire cooldown, ×1.5 damage.
pub const OVERDRIVE_DURATION: f32 = 4.5;
pub const OVERDRIVE_FIRE_MULT: f32 = 0.55;
pub const OVERDRIVE_DMG_MULT: f32 = 1.5;

/// The player ship: movement tuning + marker. One per run (for now).
#[derive(Component, Debug)]
pub struct Ship {
    /// Forward acceleration (world units / sec²).
    pub thrust: f32,
    /// Hard speed cap (world units / sec).
    pub max_speed: f32,
}

impl Default for Ship {
    fn default() -> Self {
        Self {
            thrust: 1100.0,
            max_speed: 520.0,
        }
    }
}

/// Per-frame player intent written by `systems::input` and consumed by the
/// sim. Decouples device handling from gameplay (mirrors the role of
/// `js/modules/ui/input-handler.js`). Phase 4 adds mouse-aim + gamepad.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Intent {
    /// Screen-space move direction (WASD / left stick), length ≤ 1.
    pub move_dir: Vec2,
    /// World-space aim point (mouse cursor), valid while `aim_active`.
    pub aim: Vec2,
    /// True when mouse-aim is driving the ship (cursor in-window): the hull
    /// turns toward `aim` instead of strafe-rotating.
    pub aim_active: bool,
    /// Primary fire held.
    pub firing: bool,
}

/// Per-ship fire-cooldown timer. The actual primary/power weapon stats live in
/// `systems::weapons` (`CurrentWeapon` + the weapon table); this only tracks
/// when the commander may next fire.
#[derive(Component, Debug, Default)]
pub struct Weapon {
    /// Counts down to 0; ready to fire at 0.
    pub timer: f32,
}
