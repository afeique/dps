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
}

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

/// The player ship: movement tuning + marker. One per run (for now).
#[derive(Component, Debug)]
pub struct Ship {
    /// Forward acceleration (world units / sec²).
    pub thrust: f32,
    /// Hard speed cap (world units / sec).
    pub max_speed: f32,
    /// Rotation rate from strafe input (rad / sec).
    pub turn_rate: f32,
}

impl Default for Ship {
    fn default() -> Self {
        Self {
            thrust: 1100.0,
            max_speed: 520.0,
            turn_rate: 4.5,
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

/// Primary weapon state. The 5 primary + 5 power weapons (with multishot,
/// homing, etc.) are ported in Phase 3 from `combat/weapon-data.js`.
#[derive(Component, Debug)]
pub struct Weapon {
    /// Seconds between shots.
    pub cooldown: f32,
    /// Counts down to 0; ready to fire at 0.
    pub timer: f32,
    pub bullet_speed: f32,
    pub damage: f32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            cooldown: 0.12,
            timer: 0.0,
            bullet_speed: 950.0,
            damage: 10.0,
        }
    }
}
