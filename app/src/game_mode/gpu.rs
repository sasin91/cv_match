//! A game by what it does rather than by its name: a program keeps the graphics card's 3D engine
//! busy, in front or behind the window the user switched to. Windows counts each process's share of each engine (the "GPU Engine" counters
//! Task Manager's GPU column shows), one counter instance per process and engine, named like
//! `pid_1234_luid_0x0000_0x0000D8A4_phys_0_eng_0_engtype_3D`; windows::GpuWatch reads them,
//! averaged since its last look, and heavy_app weighs them here.
//!
//! Blender or a video editor counts too, which is as it should be: they need the card as much.
//! Linux has no per-process counter, only the whole card's load, which JobFinder's own
//! llama-server is part of, so there the other signals stay the only ones.

use super::Playing;

/// The share of the 3D engine a program keeps busy, averaged since the last look, from
/// which it counts as a game (a quarter): a browser, a video or the desktop stays well under,
/// any game well over.
pub const GAME_SHARE: f64 = 25.0;

/// The process ID of a 3D engine's counter instance, or None for another engine's.
pub fn three_d_process(instance: &str) -> Option<u32> {
    if !instance.to_ascii_lowercase().ends_with("engtype_3d") {
        return None;
    }
    instance.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

/// Programs that keep the 3D engine busy without being games, by file name: Windows' own
/// desktop compositor, and the model servers this app or the user runs (whose load is the very
/// work that should make way).
const NOT_GAMES: [&str; 6] = [
    "dwm.exe",
    "llama-server.exe",
    "ollama.exe",
    "ollama_llama_server.exe",
    "ollama app.exe",
    "lm studio.exe",
];

/// Each process's share of the 3D engine, adding its counter instances up (one per adapter
/// and engine), from (instance, share) pairs, the busiest first.
pub fn busiest_processes<'a>(shares: impl IntoIterator<Item = (&'a str, f64)>) -> Vec<(u32, f64)> {
    let mut per_process: Vec<(u32, f64)> = Vec::new();
    for (instance, share) in shares {
        let Some(process_id) = three_d_process(instance) else {
            continue;
        };
        match per_process.iter_mut().find(|(id, _)| *id == process_id) {
            Some((_, total)) => *total += share,
            None => per_process.push((process_id, share)),
        }
    }
    per_process.sort_by(|a, b| b.1.total_cmp(&a.1));
    per_process
}

/// program (its file's name) as a game, when its share is GAME_SHARE or more and it isn't one of
/// NOT_GAMES.
pub fn heavy_app(program: &str, share: f64) -> Option<Playing> {
    let not_a_game = NOT_GAMES.iter().any(|name| name.eq_ignore_ascii_case(program));
    (share >= GAME_SHARE && !not_a_game).then(|| Playing::GpuHeavy {
        program: program.to_string(),
        // Whole percents are enough for the log.
        share: share.round() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBLOX_3D: &str = "pid_4242_luid_0x00000000_0x0000D8A4_phys_0_eng_0_engtype_3D";
    const ROBLOX_COPY: &str = "pid_4242_luid_0x00000000_0x0000D8A4_phys_0_eng_2_engtype_Copy";
    const BROWSER_3D: &str = "pid_77_luid_0x00000000_0x0000D8A4_phys_0_eng_0_engtype_3D";

    #[test]
    fn only_3d_engines_name_their_process() {
        assert_eq!(three_d_process(ROBLOX_3D), Some(4242));
        assert_eq!(three_d_process(ROBLOX_COPY), None);
        assert_eq!(three_d_process("_Total"), None);
    }

    #[test]
    fn a_process_share_adds_up_its_3d_engines_only_busiest_first() {
        let shares = [
            (BROWSER_3D, 10.0),
            (ROBLOX_3D, 30.0),
            (ROBLOX_COPY, 50.0),
            (ROBLOX_3D, 12.5),
            ("_Total", 90.0),
        ];
        assert_eq!(busiest_processes(shares), vec![(4242, 42.5), (77, 10.0)]);
    }

    #[test]
    fn the_desktop_and_model_servers_are_no_games() {
        assert_eq!(heavy_app("dwm.exe", 80.0), None);
        assert_eq!(heavy_app("LLAMA-SERVER.EXE", 95.0), None);
        assert_eq!(heavy_app("ollama_llama_server.exe", 95.0), None);
    }

    #[test]
    fn a_quarter_of_the_card_makes_a_game() {
        let roblox = heavy_app("RobloxPlayerBeta.exe", 61.6);
        assert_eq!(
            roblox,
            Some(Playing::GpuHeavy {
                program: "RobloxPlayerBeta.exe".to_string(),
                share: 62
            })
        );
        assert_eq!(heavy_app("msedge.exe", 8.0), None);
    }
}
