//! Trainer automation (`SHK_TRAINER_SCRIPT=<file>` or `autorun.txt` in the plugin folder): one
//! step per presented frame, for tests and reproducible set-ups.
//!
//! ```text
//! wait 10                       frames
//! feature health on|off         a toggle
//! feature add_money value 5000  a value (value features also apply it)
//! feature add_money run         an action
//! feature weather pick RAIN     a list entry by name (or index)
//! feature spawn_vehicle text adder
//! feature health expect on|off
//! cheat money on|off
//! log text
//! ```

use crate::runtime::Trainer;
use crate::script::Item;
use std::sync::Arc;

pub struct Auto {
    lines: Vec<String>,
    pos: usize,
    wait: u64,
    pub failures: u32,
    pub done: bool,
}

impl Auto {
    pub fn parse(text: &str) -> Auto {
        let lines = text.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect();
        Auto { lines, pos: 0, wait: 0, failures: 0, done: false }
    }

    /// Runs this frame's commands; returns log lines.
    pub fn step(&mut self, tr: &Arc<Trainer>) -> Vec<String> {
        let mut log = Vec::new();
        if self.done {
            return log;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return log;
        }
        while self.pos < self.lines.len() {
            let l = self.lines[self.pos].clone();
            self.pos += 1;
            if let Some(n) = l.strip_prefix("wait") {
                self.wait = n.trim().parse::<u64>().unwrap_or(1).saturating_sub(1);
                return log;
            }
            match command(tr, &l) {
                Ok(m) => log.push(format!("trainer script: {l} -> {}", if m.is_empty() { "ok" } else { &m })),
                Err(e) => {
                    self.failures += 1;
                    log.push(format!("trainer script: {l} -> FAILED: {e}"));
                }
            }
        }
        self.done = true;
        log.push(format!("trainer script: done, {} failure(s)", self.failures));
        log
    }
}

fn command(tr: &Arc<Trainer>, l: &str) -> Result<String, String> {
    let w: Vec<&str> = l.split_whitespace().collect();
    let arg = |k: usize| w.get(k).copied().ok_or_else(|| format!("missing argument {k}"));
    match w[0] {
        "log" => Ok(w[1..].join(" ")),
        "feature" => {
            let i = tr.index(arg(1)?).ok_or("no such feature")?;
            match arg(2)? {
                "on" | "off" => tr.set(i, arg(2)? == "on").map(|_| String::new()),
                "run" => tr.run(i, None).map(|_| "started".into()),
                "value" => {
                    tr.set_value(i, arg(3)?.parse::<f64>().map_err(|_| "not a number")?);
                    // a value feature applies its value (as the menu does on change)
                    if tr.feature(i).kind == crate::features::FKind::Value {
                        tr.run(i, None)?;
                        return Ok("applied".into());
                    }
                    Ok(String::new())
                }
                "pick" => {
                    let l = tr.feature(i).list.clone().ok_or("not a list feature")?;
                    let items = tr.lists.lock().unwrap_or_else(|e| e.into_inner()).get(&l).cloned().unwrap_or_default();
                    let want = w[3..].join(" ");
                    let it = want.parse::<usize>().ok().and_then(|k| items.get(k).cloned()).or_else(|| items.iter().find(|x| x.name.eq_ignore_ascii_case(&want) || x.label.eq_ignore_ascii_case(&want)).cloned()).ok_or_else(|| format!("no '{want}' in the list {l}"))?;
                    tr.run(i, Some(it)).map(|_| "started".into())
                }
                "text" => {
                    let t = w[3..].join(" ");
                    tr.run(i, Some(Item { name: t.clone(), label: t, ..Default::default() })).map(|_| "started".into())
                }
                "expect" => {
                    let want = arg(3)? == "on";
                    if tr.is_on(i) == want {
                        Ok("as expected".into())
                    } else {
                        Err(format!("is {}", if want { "off" } else { "on" }))
                    }
                }
                o => Err(format!("unknown feature command {o}")),
            }
        }
        "cheat" => {
            let c = tr.cheats.cheats.iter().position(|c| c.id == arg(1).unwrap_or("")).ok_or("no such cheat")?;
            if let Some(r) = tr.cheats.cheats[c].unusable() {
                return Err(r);
            }
            tr.cheat_switch(c, arg(2)? == "on");
            Ok(String::new())
        }
        o => Err(format!("unknown command {o}")),
    }
}
