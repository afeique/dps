//! Background music player.
//!
//! Shuffle-plays all `*.mp3` files found in `<CARGO_MANIFEST_DIR>/music/` at startup,
//! auto-advancing to the next track when one finishes, looping the playlist forever.
//!
//! # Wire-up (parent must do)
//! ```rust
//! mod music;
//! // inside App::new() builder:
//! app.add_plugins(music::MusicPlugin);
//! ```
//! Cargo.toml must also enable the `mp3` feature on `bevy`:
//! ```toml
//! bevy = { version = "0.18", features = ["mp3", ...] }
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

// ── Marker component ─────────────────────────────────────────────────────────

/// Marker placed on the entity that is currently playing music.
/// When the track finishes Bevy despawns the entity (via `PlaybackMode::Despawn`),
/// which removes this marker and triggers the auto-advance system.
#[derive(Component)]
pub struct CurrentMusic;

// ── Resource ─────────────────────────────────────────────────────────────────

/// Holds the shuffled list of `*.mp3` `PathBuf`s and the current playhead index.
#[derive(Resource)]
pub struct MusicPlaylist {
    pub tracks: Vec<PathBuf>,
    pub index:  usize,
}

// ── Tiny PRNG (xorshift64) — no `rand` crate needed ─────────────────────────

/// Seeds a 64-bit xorshift PRNG from the current system time.
fn prng_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs().wrapping_mul(6_364_136_223_846_793_005)))
        .unwrap_or(0xDEAD_BEEF_CAFE_1234);
    // Ensure a non-zero seed (xorshift64 requires non-zero state).
    if nanos == 0 { 0xDEAD_BEEF_CAFE_1234 } else { nanos }
}

/// One step of a 64-bit xorshift PRNG.
#[inline(always)]
fn xorshift64(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Fisher-Yates shuffle using the xorshift64 PRNG.
fn shuffle<T>(slice: &mut [T], rng: &mut u64) {
    let n = slice.len();
    for i in (1..n).rev() {
        let j = (xorshift64(rng) as usize) % (i + 1);
        slice.swap(i, j);
    }
}

// ── Startup system ───────────────────────────────────────────────────────────

/// Scans `CARGO_MANIFEST_DIR/music/*.mp3`, shuffles the list with Fisher-Yates,
/// inserts a `MusicPlaylist` resource, and immediately spawns the first track.
fn setup_music(
    mut commands: Commands,
    mut sources:  ResMut<Assets<AudioSource>>,
) {
    let music_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/music");

    // Collect *.mp3 paths from the music directory.
    let mut tracks: Vec<PathBuf> = match std::fs::read_dir(music_dir) {
        Ok(rd) => rd
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("mp3") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect(),
        Err(e) => {
            warn!("MusicPlugin: cannot read music dir `{music_dir}`: {e}");
            commands.insert_resource(MusicPlaylist { tracks: vec![], index: 0 });
            return;
        }
    };

    if tracks.is_empty() {
        warn!("MusicPlugin: no *.mp3 files found in `{music_dir}` — music disabled");
        commands.insert_resource(MusicPlaylist { tracks: vec![], index: 0 });
        return;
    }

    // Sort first for deterministic ordering before shuffling (read_dir order is arbitrary).
    tracks.sort();

    // Fisher-Yates shuffle.
    let mut rng = prng_seed();
    shuffle(&mut tracks, &mut rng);

    info!("MusicPlugin: found {} tracks, starting playback", tracks.len());

    // Play the first track immediately.
    spawn_track(&tracks[0], &mut commands, &mut sources);

    commands.insert_resource(MusicPlaylist { tracks, index: 0 });
}

// ── Per-frame auto-advance system ────────────────────────────────────────────

/// When the `CurrentMusic` entity disappears (despawned at track end), advance the
/// playlist index and spawn the next track.
fn advance_music(
    mut commands: Commands,
    mut sources:  ResMut<Assets<AudioSource>>,
    mut playlist: ResMut<MusicPlaylist>,
    music_query:  Query<(), With<CurrentMusic>>,
) {
    // A non-empty playlist with no active music entity means the track just ended.
    if playlist.tracks.is_empty() || !music_query.is_empty() {
        return;
    }

    // Bump index, wrapping around.
    let next = playlist.index.wrapping_add(1) % playlist.tracks.len();
    playlist.index = next;

    let path = playlist.tracks[next].clone();
    spawn_track(&path, &mut commands, &mut sources);
}

// ── Shared helper ────────────────────────────────────────────────────────────

/// Read the mp3 file at `path` into an `AudioSource` asset, then spawn an
/// `AudioPlayer` entity tagged with `CurrentMusic`.
///
/// Mirrors the `AudioSource { bytes: Arc::from(...) }` pattern from `src/audio.rs`.
fn spawn_track(
    path:     &std::path::Path,
    commands: &mut Commands,
    sources:  &mut ResMut<Assets<AudioSource>>,
) {
    match std::fs::read(path) {
        Ok(bytes) => {
            let source = AudioSource {
                bytes: Arc::from(bytes.as_slice()),
            };
            let handle = sources.add(source);

            commands.spawn((
                AudioPlayer::new(handle),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
                CurrentMusic,
            ));

            // Log only the file name for brevity.
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>");
            debug!("MusicPlugin: playing `{name}`");
        }
        Err(e) => {
            let name = path.display();
            warn!("MusicPlugin: failed to read `{name}`: {e} — skipping");
        }
    }
}

// ── Plugin ───────────────────────────────────────────────────────────────────

/// Bevy plugin that shuffle-plays local `*.mp3` files from `<crate-root>/music/`.
///
/// Add to your app with:
/// ```rust
/// app.add_plugins(music::MusicPlugin);
/// ```
pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_music)
           .add_systems(Update, advance_music);
    }
}
