//! Automation of the overlay for tests and screenshots: `SHK_AUTOMATION=<file>`, one step per
//! presented frame. Commands: `wait N` (frames), `key NAME` (a key press), `capture FILE` (the
//! next presented image with the overlay, PNG), `stats` (a log line), `log TEXT`.

use crate::{host, input, log, overlay, scripts};
use std::sync::Mutex;

struct Auto {
    lines: Vec<String>,
    pos: usize,
    wait: u64,
}

static AUTO: Mutex<Option<Auto>> = Mutex::new(None);

pub fn init() {
    let Ok(p) = std::env::var("SHK_AUTOMATION") else { return };
    match std::fs::read_to_string(&p) {
        Ok(t) => {
            let lines = t.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect();
            *AUTO.lock().unwrap_or_else(|e| e.into_inner()) = Some(Auto { lines, pos: 0, wait: 0 });
        }
        Err(e) => log::warn(&format!("automation: {p}: {e}")),
    }
}

pub fn step() {
    let mut g = AUTO.lock().unwrap_or_else(|e| e.into_inner());
    let Some(a) = g.as_mut() else { return };
    if a.wait > 0 {
        a.wait -= 1;
        return;
    }
    while a.pos < a.lines.len() {
        let l = a.lines[a.pos].clone();
        a.pos += 1;
        let (cmd, rest) = l.split_once(' ').unwrap_or((l.as_str(), ""));
        match cmd {
            "wait" => {
                a.wait = rest.trim().parse::<u64>().unwrap_or(1).saturating_sub(1);
                return;
            }
            "key" => match scripthook_sdk::keys::vk_from_name(rest) {
                Some(vk) => {
                    input::inject_key(vk);
                    // one key per frame
                    return;
                }
                None => log::warn(&format!("automation: unknown key {rest}")),
            },
            "capture" => overlay::capture_next(rest.trim().into()),
            "stats" => {
                let (c, f) = host::native_stats();
                log::info(&format!("automation: stats presents={} drawn={} ticks={} mode={:?} natives={c} failed={f} plugins={}", overlay::PRESENTS.load(std::sync::atomic::Ordering::Relaxed), overlay::DRAWN.load(std::sync::atomic::Ordering::Relaxed), scripts::ticks(), scripts::mode(), crate::plugins::names().join(",")));
            }
            "log" => log::info(&format!("automation: {rest}")),
            o => log::warn(&format!("automation: unknown command {o}")),
        }
    }
    log::info("automation: done");
    *g = None;
}
