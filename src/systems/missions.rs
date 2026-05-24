//! Per-wave missions (spec V.6). Each wave assigns one objective; completing it
//! grants a reward. The JS reward is **+1 SP** — meta progression this port
//! retired — so we substitute a small wave-scaled **gold** bonus (flagged here as
//! a deliberate divergence). Boss waves are always `NoDamage`; other waves roll
//! among the trackable objectives.
//!
//! `precision` (25 crits) from the spec is **deferred** — it needs a per-crit
//! event we don't emit yet; the roll covers the other four.

use crate::messages::{Death, PlayerHurt};
use crate::resources::{GameRng, KillStreak, Score};
use crate::systems::asteroids::Asteroid;
use crate::systems::wave::Wave;
use bevy::prelude::*;

/// A per-wave objective.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MissionKind {
    NoDamage,
    FastKill,
    Asteroid,
    Streak,
}

impl MissionKind {
    /// HUD label.
    pub fn label(self) -> &'static str {
        match self {
            Self::NoDamage => "Take no damage",
            Self::FastKill => "5 kills in 8s",
            Self::Asteroid => "Clear all asteroids",
            Self::Streak => "Reach a 12 kill-streak",
        }
    }
}

/// `FastKill` window (spec V.6: 5 kills / 8 s).
const FAST_KILL_WINDOW: f32 = 8.0;
const FAST_KILL_COUNT: usize = 5;
/// `Streak` target (spec V.6: 12-streak).
const STREAK_TARGET: u32 = 12;

/// Wave-scaled gold bonus standing in for the JS +1 SP reward.
pub fn mission_reward(wave: usize) -> u64 {
    30 + wave as u64 * 8
}

/// Boss waves (`[3,6,…,30]`, every 3rd) always assign `NoDamage`; others roll one
/// of the four trackable objectives.
pub fn mission_for_wave(wave: usize, rng: &mut GameRng) -> MissionKind {
    if wave % 3 == 0 {
        return MissionKind::NoDamage;
    }
    match (rng.next_f32() * 4.0) as u32 {
        0 => MissionKind::NoDamage,
        1 => MissionKind::FastKill,
        2 => MissionKind::Asteroid,
        _ => MissionKind::Streak,
    }
}

/// The active wave's mission + its tracking state. `init_resource` in app.rs.
#[derive(Resource)]
pub struct Mission {
    pub kind: MissionKind,
    pub done: bool,
    took_damage: bool,
    peak_streak: u32,
    /// Kill timestamps (s) within the fast-kill window.
    kill_times: Vec<f32>,
    saw_asteroid: bool,
}

impl Default for Mission {
    fn default() -> Self {
        Self {
            kind: MissionKind::NoDamage,
            done: false,
            took_damage: false,
            peak_streak: 0,
            kill_times: Vec::new(),
            saw_asteroid: false,
        }
    }
}

impl Mission {
    fn reset(&mut self, kind: MissionKind) {
        self.kind = kind;
        self.done = false;
        self.took_damage = false;
        self.peak_streak = 0;
        self.kill_times.clear();
        self.saw_asteroid = false;
    }
}

/// Assign the mission at wave start, track progress, and grant the gold reward on
/// completion (spec V.6). `NoDamage` is settled at wave clear (you only know you
/// took no damage once the wave ends); the other objectives complete mid-wave.
#[allow(clippy::too_many_arguments)]
pub fn update_missions(
    time: Res<Time>,
    mut last_wave: Local<Option<usize>>,
    mut mission: ResMut<Mission>,
    mut rng: ResMut<GameRng>,
    wave: Res<Wave>,
    streak: Res<KillStreak>,
    mut score: ResMut<Score>,
    mut hurt: MessageReader<PlayerHurt>,
    mut deaths: MessageReader<Death>,
    asteroids: Query<(), With<Asteroid>>,
) {
    let now = time.elapsed_secs();
    let w = wave.number();

    // Wave changed → settle the old mission, assign the new one.
    if *last_wave != Some(w) {
        // Settle NoDamage at wave clear (other kinds settle mid-wave below).
        if last_wave.is_some()
            && mission.kind == MissionKind::NoDamage
            && !mission.done
            && !mission.took_damage
        {
            score.gold += mission_reward(last_wave.unwrap());
        }
        let kind = mission_for_wave(w, &mut rng);
        mission.reset(kind);
        *last_wave = Some(w);
    }

    // Track observable progress.
    if hurt.read().count() > 0 {
        mission.took_damage = true;
    }
    for d in deaths.read() {
        if d.kind.is_some() {
            mission.kill_times.push(now); // enemy kill
        }
    }
    mission.kill_times.retain(|t| now - *t <= FAST_KILL_WINDOW);
    mission.peak_streak = mission.peak_streak.max(streak.kills);
    let asteroid_count = asteroids.iter().count();
    if asteroid_count > 0 {
        mission.saw_asteroid = true;
    }

    // Mid-wave completion for the three immediate objectives.
    if !mission.done {
        let complete = match mission.kind {
            MissionKind::Streak => mission.peak_streak >= STREAK_TARGET,
            MissionKind::FastKill => mission.kill_times.len() >= FAST_KILL_COUNT,
            MissionKind::Asteroid => mission.saw_asteroid && asteroid_count == 0,
            MissionKind::NoDamage => false, // settled at wave clear
        };
        if complete {
            mission.done = true;
            score.gold += mission_reward(w);
        }
    }
}
