//! The menu itself: model + navigation + the menu's own pages (Settings, About) + drawing.
//! Used by the plugin when the host can draw, and directly by tests and the preview tool.

use crate::model::{ItemId, Model, PageId, ROOT};
use crate::nav::{filtered, Action, Nav, Navigator, Row};
use crate::render::{self, Theme, View};
use scripthook_sdk::draw::Painter;
use scripthook_sdk::ffi::*;
use scripthook_sdk::menu::{ItemSpec, Kind};
use std::ffi::{c_void, CString};

/// Parent of the menu's own pages (they are listed after the plugins' pages on the root).
pub const OWN: PageId = u32::MAX;

#[derive(Debug, Clone)]
pub struct Settings {
    pub right_side: bool,
    /// 0 = automatic
    pub scale: f32,
    pub show_hud: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { right_side: false, scale: 0.0, show_hud: true }
    }
}

/// An event to deliver to an item's owner (outside every lock).
pub struct Dispatch {
    pub cb: ShkMenuEventFn,
    pub user: usize,
    pub ty: u32,
    pub item: ItemId,
    pub value: f64,
    pub text: Option<CString>,
}

impl Dispatch {
    /// # Safety
    /// The owner's callback must still be loaded (owners remove their items at unload).
    pub unsafe fn run(&self) {
        let ev = ShkMenuEvent { size: std::mem::size_of::<ShkMenuEvent>() as u32, ty: self.ty, item: self.item, reserved: 0, value: self.value, text: self.text.as_ref().map(|t| t.as_ptr()).unwrap_or(std::ptr::null()) };
        (self.cb)(self.user as *mut c_void, &ev);
    }
}

pub struct MenuCore {
    pub model: Model,
    pub nav: Navigator,
    pub theme: Theme,
    pub settings: Settings,
    pub title: String,
    pub version: String,
    pub host_name: String,
    settings_page: PageId,
    about_page: PageId,
    s_side: ItemId,
    s_scale: ItemId,
    s_hud: ItemId,
    /// the open key's name for the footer
    pub open_key: String,
}

impl MenuCore {
    pub fn new(host_name: &str) -> MenuCore {
        let mut model = Model::new();
        // the menu's own pages: owner 0, parent OWN (not listed among the plugins' pages)
        let settings_page = model.add_page(0, ROOT, "Settings", "How the menu looks.");
        let about_page = model.add_page(0, ROOT, "About", "");
        for p in model.pages.iter_mut().filter(|p| p.owner == 0) {
            p.parent = OWN;
        }
        let st = Settings::default();
        let s_side = model.add_item(0, settings_page, ItemSpec::choice("Menu position", vec!["Left".into(), "Right".into()], st.right_side as usize).hint("Which side of the screen the menu is drawn on."), None);
        let s_scale = model.add_item(0, settings_page, ItemSpec::choice("Text size", vec!["Automatic".into(), "1x".into(), "2x".into(), "3x".into()], 0).hint("Automatic follows the screen height: 1x below 600 lines, 2x up to 1300, 3x above."), None);
        let s_hud = model.add_item(0, settings_page, ItemSpec::toggle("Show HUD lines", st.show_hud).hint("Lines plugins show at the top right (coordinates, FPS, ...)."), None);
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        for (l, h) in [
            (format!("gta-vi-scripthook {version}"), "The script hook's plugin SDK, this menu and the trainer. MIT OR Apache-2.0."),
            (format!("Host: {host_name}"), "The program that runs the game and loads the plugins."),
            ("Unofficial fan project".to_string(), "Not affiliated with or endorsed by Rockstar Games or Take-Two Interactive."),
            ("Single-player / offline only".to_string(), "Nothing here touches online features, protection or licence checks."),
            ("Requires your own copy of the game".to_string(), "This project contains no game files and no data taken from the game."),
        ] {
            model.add_item(0, about_page, ItemSpec::text(l).hint(h), None);
        }
        MenuCore {
            model,
            nav: Navigator::default(),
            theme: Theme::default(),
            settings: st,
            title: "gta-vi-scripthook".into(),
            version,
            host_name: host_name.to_string(),
            settings_page,
            about_page,
            s_side,
            s_scale,
            s_hud,
            open_key: "F4".into(),
        }
    }

    pub fn apply_settings(&mut self, s: Settings) {
        self.model.set_value(self.s_side, s.right_side as i32 as f64);
        self.model.set_value(self.s_scale, s.scale.clamp(0.0, 3.0).round() as f64);
        self.model.set_value(self.s_hud, s.show_hud as i32 as f64);
        self.settings = s;
    }

    fn row_of(&self, id: ItemId) -> Option<Row> {
        self.model.item(id).map(|i| Row { item: Some(i.id), spec: i.spec.clone() })
    }

    /// Rows of the current page (filter applied).
    pub fn rows(&self) -> Vec<Row> {
        let page = self.nav.page();
        if page == ROOT {
            let mut v: Vec<Row> = Vec::new();
            let roots = self.model.roots();
            if roots.is_empty() {
                v.push(Row { item: None, spec: ItemSpec::text("No plugin pages yet").hint("Plugins add their pages through the menu service; the trainer appears here when it is installed.") });
            }
            for p in roots {
                v.push(Row { item: None, spec: ItemSpec::subpage(p.title.clone(), p.id).hint(p.subtitle.clone()) });
            }
            v.push(Row { item: None, spec: ItemSpec::separator("Menu") });
            v.push(Row { item: None, spec: ItemSpec::subpage("Settings", self.settings_page).hint("Menu position, text size, HUD lines.") });
            v.push(Row { item: None, spec: ItemSpec::subpage("About", self.about_page).hint("Version, host, and what this project is (and is not).") });
            return v;
        }
        let Some(p) = self.model.page(page) else { return Vec::new() };
        let rows: Vec<Row> = p.items.iter().filter_map(|&i| self.row_of(i)).collect();
        if p.filterable {
            filtered(rows, self.nav.filter())
        } else {
            rows
        }
    }

    pub fn filterable(&self) -> bool {
        self.model.page(self.nav.page()).map(|p| p.filterable).unwrap_or(false)
    }

    pub fn set_open(&mut self, on: bool) {
        self.nav.set_open(on);
    }

    /// Applies navigation; returns the events for the owners.
    pub fn handle(&mut self, navs: &[Nav]) -> Vec<Dispatch> {
        if !self.nav.open || navs.is_empty() {
            return Vec::new();
        }
        // a page that disappeared (its plugin unloaded): back to the root
        if self.nav.page() != ROOT && self.model.page(self.nav.page()).is_none() {
            self.nav.reset();
        }
        let mut out = Vec::new();
        for &n in navs {
            let rows = self.rows();
            let filterable = self.filterable();
            for a in self.nav.handle(&[n], &rows, filterable) {
                let (item, ty, value, text) = match a {
                    Action::Change(Some(i), v) => {
                        self.model.set_value(i, v);
                        (i, SHK_EVENT_CHANGE, v, None)
                    }
                    Action::Text(Some(i), t) => {
                        if let Some(it) = self.model.item_mut(i) {
                            it.spec.text = t.clone();
                        }
                        (i, SHK_EVENT_TEXT, 0.0, Some(t))
                    }
                    Action::Activate(Some(i)) => (i, SHK_EVENT_ACTIVATE, 0.0, None),
                    _ => continue,
                };
                if item == self.s_side || item == self.s_scale || item == self.s_hud {
                    self.settings.right_side = self.model.item(self.s_side).map(|i| i.spec.value != 0.0).unwrap_or(false);
                    self.settings.scale = self.model.item(self.s_scale).map(|i| i.spec.value as f32).unwrap_or(0.0);
                    self.settings.show_hud = self.model.item(self.s_hud).map(|i| i.spec.value != 0.0).unwrap_or(true);
                    continue;
                }
                if let Some((cb, user)) = self.model.item(item).and_then(|i| i.cb) {
                    out.push(Dispatch { cb, user, ty, item, value, text: text.map(|t| CString::new(t.replace('\0', "")).unwrap_or_default()) });
                }
            }
        }
        out
    }

    fn scale(&self, p: &dyn Painter) -> f32 {
        if self.settings.scale >= 1.0 {
            self.settings.scale
        } else {
            p.ui_scale()
        }
    }

    /// Draws HUD lines, the open menu and the toasts.
    pub fn draw(&mut self, p: &mut dyn Painter, now: f64) {
        self.model.expire(now);
        let scale = self.scale(p);
        if self.settings.show_hud && !self.model.hud.is_empty() && !(self.nav.open && self.settings.right_side) {
            let lines: Vec<String> = self.model.hud.iter().map(|h| h.2.clone()).collect();
            render::draw_hud(p, &self.theme, scale, &lines);
        }
        if self.nav.open {
            if self.nav.page() != ROOT && self.model.page(self.nav.page()).is_none() {
                self.nav.reset();
            }
            let rows = self.rows();
            self.nav.fix(&rows);
            let page = self.nav.page();
            let subtitle = self.model.page(page).map(|p| p.subtitle.clone()).unwrap_or_else(|| format!("Host: {}", self.host_name));
            let key = self.open_key.clone();
            let hints: Vec<(&str, &str)> = if self.nav.editing() {
                vec![("Enter", "ok"), ("Esc", "cancel"), ("<>", "step")]
            } else {
                vec![("Enter", "select"), ("<>", "change"), ("Esc", "back"), (key.as_str(), "close")]
            };
            let view = View { title: &self.title, version: &self.version, breadcrumb: self.model.breadcrumb(page), subtitle, filterable: self.filterable(), hints: &hints, right_side: self.settings.right_side, scale, time: now };
            let vis = render::draw_menu(p, &self.theme, &view, &self.nav, &rows);
            self.nav.visible = vis;
        }
        let toasts: Vec<(String, f64)> = self.model.toasts.iter().map(|t| (t.text.clone(), t.until - now)).collect();
        if !toasts.is_empty() {
            render::draw_toasts(p, &self.theme, scale, &toasts);
        }
    }

    /// For tests: the label of the selected row.
    pub fn selected_label(&self) -> Option<String> {
        self.rows().get(self.nav.selected()).map(|r| r.spec.label.clone())
    }

    pub fn kind_of(&self, item: ItemId) -> Option<Kind> {
        self.model.item(item).map(|i| i.spec.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scripthook_sdk::draw::SoftCanvas;
    use std::sync::Mutex;

    static EVENTS: Mutex<Vec<(u32, u32, f64, String)>> = Mutex::new(Vec::new());

    unsafe extern "C" fn record(_user: *mut c_void, e: *const ShkMenuEvent) {
        let e = &*e;
        let t = if e.text.is_null() { String::new() } else { std::ffi::CStr::from_ptr(e.text).to_string_lossy().into_owned() };
        EVENTS.lock().unwrap().push((e.ty, e.item, e.value, t));
    }

    #[test]
    fn menu_flow_events_and_drawing() {
        let mut m = MenuCore::new("test host");
        let tr = m.model.add_page(5, ROOT, "Trainer", "Features on natives and memory cheats.");
        let player = m.model.add_page(5, tr, "Player", "");
        let cb = Some((record as ShkMenuEventFn, 0usize));
        m.model.add_item(5, tr, ItemSpec::subpage("Player", player), cb);
        let god = m.model.add_item(5, player, ItemSpec::toggle("Invincible", false).hint("No damage."), cb);
        let money = m.model.add_item(5, player, ItemSpec::button("Add money"), cb);
        m.model.add_item(5, player, ItemSpec::toggle("Unlimited money", false).disabled("needs native STATS::STAT_SET_INT (unknown until the executable)"), cb);
        m.set_open(true);
        assert_eq!(m.selected_label().as_deref(), Some("Trainer"));
        let d = m.handle(&[Nav::Accept, Nav::Accept, Nav::Accept]);
        assert_eq!(m.nav.page(), player);
        assert_eq!(d.len(), 1);
        unsafe { d[0].run() };
        let d = m.handle(&[Nav::Down, Nav::Accept]);
        unsafe { d.iter().for_each(|x| x.run()) };
        let ev = EVENTS.lock().unwrap().clone();
        assert_eq!(ev, vec![(SHK_EVENT_CHANGE, god, 1.0, String::new()), (SHK_EVENT_ACTIVATE, money, 0.0, String::new())]);
        assert_eq!(m.model.item(god).unwrap().spec.value, 1.0);
        // disabled: no event
        assert!(m.handle(&[Nav::Down, Nav::Accept]).is_empty());
        // drawing: header at the top left, accent strip, selection row
        let mut c = SoftCanvas::new(1280, 720, 0x00203040);
        m.model.set_hud(5, 0, "FPS 60");
        m.model.toast("spawned adder", 3.0, 0.0);
        m.draw(&mut c, 1.0);
        let th = &m.theme;
        let px = c.pixel(12 * 2 + 40, 12 * 2 + 5);
        let want = th.header;
        assert!((px[0] as i32 - ((want >> 16) & 0xFF) as i32).abs() < 12, "header colour at the top left: {px:?}");
        let acc = c.pixel(12 * 2 + 100, 12 * 2 + 22 * 2 - 1);
        assert_eq!(acc[1], ((th.accent >> 8) & 0xFF) as u8, "accent strip under the header");
        assert!(m.nav.visible >= 4);
        // settings change the side
        m.nav.reset();
        let rows = m.rows();
        let k = rows.iter().position(|r| r.spec.label == "Settings").unwrap();
        m.nav.stack[0].sel = k;
        m.handle(&[Nav::Accept, Nav::Right]);
        assert!(m.settings.right_side);
        // a plugin unloading while its page is open: back to the root
        m.nav.reset();
        m.handle(&[Nav::Home, Nav::Accept]);
        assert_eq!(m.nav.page(), tr);
        m.model.remove_owner(5);
        m.handle(&[Nav::Down]);
        assert_eq!(m.nav.page(), ROOT);
    }
}
