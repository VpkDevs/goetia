//! DEMONICON — cargo run -p demonicon
//! Flags: --seed N   --novsync   --frames N (bench + exit)

use demonicon::game::Demonicon;
use goetia::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<u64> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
    };
    let config = AppConfig {
        title: "DEMONICON".into(),
        size: (1600, 900),
        vsync: !args.iter().any(|a| a == "--novsync"),
        max_frames: get("--frames"),
        master_seed: get("--seed").unwrap_or_else(|| {
            // Fresh realm sequence per profile launch; determinism within a
            // seed remains absolute (pass --seed to reproduce a run).
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(666)
        }),
        threads: 0,
    };
    let mut game = Demonicon::new();
    if let Some(d) = get("--demon") {
        game.autostart = Some((d as usize, get("--tier").unwrap_or(1) as u32));
    }
    if let Err(e) = App::run(config, game) {
        eprintln!("fatal: {e}");
        std::process::exit(1);
    }
}
