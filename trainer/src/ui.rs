//! The trainer's menu pages, through the menu service (gta-vi-scripthook menu plugin) or the
//! host's basic menu.

use crate::features::{FKind, GROUPS};
use crate::runtime::{Backend, Trainer};
use crate::script::Item;
use scripthook_sdk::menu::{EventKind, ItemSpec, Menu, MenuEvent};
use std::sync::Arc;

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Most list items shown in the host's basic menu (it has no filter row).
pub const BASIC_LIST_LIMIT: usize = 500;

fn typed_item(t: &str) -> Item {
    let t = t.trim();
    let hash = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).and_then(|h| u32::from_str_radix(h, 16).ok());
    Item { name: t.to_string(), label: t.to_string(), hash, pos: None, heading: None }
}

fn report(tr: &Trainer, r: Result<(), String>) {
    if let Err(e) = r {
        tr.note(&e);
    }
}

/// Builds the pages. Returns the root page.
pub fn build(tr: &Arc<Trainer>, menu: &Menu) -> u32 {
    let rich = menu.is_service();
    let root = menu.add_page(0, "Trainer", &format!("{} of {} features usable. Single-player only.", tr.usable_count(), tr.defs.features.len()));
    let status = shk_status();
    menu.add_item(root, &ItemSpec::text(format!("Script hook: {status}")).right(format!("{} usable", tr.usable_count())).hint(tr.summary()), |_| {});
    let mut group_pages = Vec::new();
    for g in GROUPS {
        let p = menu.add_page(root, g, "");
        group_pages.push((g, p));
        if rich {
            menu.add_item(root, &ItemSpec::subpage(g, p).hint(format!("{} features", tr.defs.features.iter().filter(|f| f.group == g).count())), |_| {});
        }
    }
    for (i, f) in tr.defs.features.iter().enumerate() {
        let Some(&(_, page)) = group_pages.iter().find(|(g, _)| *g == f.group) else { continue };
        let a = &tr.avail[i];
        let hint = {
            let mut h = a.detail.clone();
            if let Backend::Cheat(_) = a.backend {
                h = format!("(natives unknown: using the memory cheat) {h}");
            }
            h
        };
        if let Some(r) = &a.reason {
            menu.add_item(page, &ItemSpec::new(if f.kind == FKind::Toggle { scripthook_sdk::menu::Kind::Toggle } else { scripthook_sdk::menu::Kind::Button }, f.name.clone()).disabled(r.clone()).hint(hint), |_| {});
            continue;
        }
        let hk = f.hotkey.map(|h| h.0).unwrap_or(0);
        let value_spec = |label: String| {
            let (lo, hi, st) = f.slider.unwrap_or((0.0, 1.0, 1.0));
            ItemSpec::slider(label, tr.value(i), lo, hi, st)
        };
        match f.kind {
            FKind::Toggle => {
                let t2 = tr.clone();
                let id = menu.add_item(page, &ItemSpec::toggle(f.name.clone(), tr.is_on(i)).hint(hint).hotkey(hk), move |e: &MenuEvent| {
                    if e.kind == EventKind::Change {
                        report(&t2, t2.set(i, e.value != 0.0));
                    }
                });
                lock(&tr.fstate)[i].toggle_item = id;
                if f.slider.is_some() {
                    let t2 = tr.clone();
                    let id = menu.add_item(page, &value_spec(format!("  {} value", f.name)), move |e| {
                        if e.kind == EventKind::Change {
                            t2.set_value(i, e.value);
                        }
                    });
                    lock(&tr.fstate)[i].value_item = id;
                }
            }
            FKind::Action => {
                if f.slider.is_some() {
                    let t2 = tr.clone();
                    let id = menu.add_item(page, &value_spec(format!("  {}: amount", f.name)), move |e| {
                        if e.kind == EventKind::Change {
                            t2.set_value(i, e.value);
                        }
                    });
                    lock(&tr.fstate)[i].value_item = id;
                }
                let t2 = tr.clone();
                menu.add_item(page, &ItemSpec::button(f.name.clone()).hint(hint).hotkey(hk), move |e| {
                    if e.kind == EventKind::Activate {
                        report(&t2, t2.run(i, None));
                    }
                });
            }
            FKind::Value => {
                let t2 = tr.clone();
                let id = menu.add_item(page, &value_spec(f.name.clone()).hint(format!("Applies on change. {hint}")), move |e| {
                    if e.kind == EventKind::Change {
                        t2.set_value(i, e.value);
                        report(&t2, t2.run(i, None));
                    }
                });
                lock(&tr.fstate)[i].value_item = id;
            }
            FKind::List => {
                let list = f.list.clone().unwrap_or_default();
                let items = lock(&tr.lists).get(&list).cloned().unwrap_or_default();
                let lp = menu.add_page(page, &f.name, &format!("{} entries of the '{list}' list{}", items.len(), if f.input { ", or type a name" } else { "" }));
                lock(&tr.fstate)[i].list_page = lp;
                let sub = ItemSpec::subpage(f.name.clone(), lp).hint(hint).right(format!("{} entries", items.len()));
                menu.add_item(page, &if rich { sub.filterable() } else { sub }, |_| {});
                if f.input {
                    let t2 = tr.clone();
                    menu.add_item(lp, &ItemSpec::input("Type a name", "").hint("A model / item name, or its 0x hash."), move |e| {
                        if e.kind == EventKind::Text && !e.text.trim().is_empty() {
                            report(&t2, t2.run(i, Some(typed_item(&e.text))));
                        }
                    });
                }
                let limit = if rich { usize::MAX } else { BASIC_LIST_LIMIT };
                for it in items.iter().take(limit) {
                    add_list_item(tr, menu, lp, i, it);
                }
                if items.len() > limit {
                    menu.add_item(lp, &ItemSpec::text(format!("({} more: use the scripthook menu for the full list and its filter)", items.len() - limit)), |_| {});
                }
            }
        }
    }
    // memory cheats no feature claims
    let cs = tr.own_cheats.clone();
    for (ci, why) in cs {
        let c = &tr.cheats.cheats[ci];
        let Some(&(_, page)) = group_pages.iter().find(|(g, _)| *g == c.group).or(group_pages.last()) else { continue };
        let hint = if c.note.is_empty() { format!("memory cheat ({})", c.kind_name) } else { format!("memory cheat ({}): {}", c.kind_name, c.note) };
        match why {
            Some(r) => {
                menu.add_item(page, &ItemSpec::toggle(c.name.clone(), false).disabled(r).hint(hint), |_| {});
            }
            None => {
                let t2 = tr.clone();
                let id = menu.add_item(page, &ItemSpec::toggle(c.name.clone(), tr.cheat_on(ci)).hint(hint), move |e| {
                    if e.kind == EventKind::Change {
                        t2.cheat_switch(ci, e.value != 0.0);
                    }
                });
                lock(&tr.cheat_items).insert(ci, id);
            }
        }
    }
    root
}

pub fn add_list_item(tr: &Arc<Trainer>, menu: &Menu, page: u32, i: usize, it: &Item) {
    let t2 = tr.clone();
    let item = it.clone();
    let label = if it.label.is_empty() { it.name.clone() } else { it.label.clone() };
    let right = match (it.pos, it.hash) {
        (Some(p), _) => format!("{:.0}, {:.0}, {:.0}", p[0], p[1], p[2]),
        (None, Some(h)) => format!("0x{h:08X}"),
        _ => String::new(),
    };
    let mut spec = ItemSpec::button(label);
    if !right.is_empty() {
        spec = spec.right(right);
    }
    menu.add_item(page, &spec, move |e| {
        if e.kind == EventKind::Activate {
            report(&t2, t2.run(i, Some(item.clone())));
        }
    });
}

fn shk_status() -> String {
    use scripthook_sdk::ScriptMode;
    match scripthook_sdk::host().script_status().mode {
        ScriptMode::GameThread => "game script thread".into(),
        ScriptMode::PresentFallback => "present-time fallback".into(),
        ScriptMode::Off => "off".into(),
    }
}
