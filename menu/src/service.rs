//! The menu service (`ShkMenuService`, "scripthook.menu") the plugin publishes. Two backends:
//! **drawn** (the host offers drawing primitives: the menu draws itself, [`MenuCore`]) and
//! **mirror** (no drawing: everything goes into the host's basic menu, [`BasicMenu`]).

use crate::core::{Dispatch, MenuCore};
use scripthook_sdk::ffi::*;
use scripthook_sdk::menu::{BasicMenu, ItemSpec};
use std::ffi::{c_char, CStr};
use std::sync::Mutex;

pub enum Backend {
    Drawn(Box<MenuCore>),
    Mirror(BasicMenu),
}

pub static BACKEND: Mutex<Option<Backend>> = Mutex::new(None);
/// Toasts in mirror mode go to the host's notices.
static NOTIFY: Mutex<Option<fn(&str, f32)>> = Mutex::new(None);

pub fn set_backend(b: Option<Backend>, notify: Option<fn(&str, f32)>) {
    *BACKEND.lock().unwrap_or_else(|e| e.into_inner()) = b;
    *NOTIFY.lock().unwrap_or_else(|e| e.into_inner()) = notify;
}

pub fn with<R>(f: impl FnOnce(&mut Backend) -> R) -> Option<R> {
    BACKEND.lock().unwrap_or_else(|e| e.into_inner()).as_mut().map(f)
}

/// Monotonic seconds (toasts).
pub fn now() -> f64 {
    scripthook_sdk::win::now()
}

unsafe fn s(p: *const c_char) -> Option<String> {
    (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
}

unsafe extern "C" fn add_page(owner: *mut ShkPlugin, parent: ShkMenuPage, title: *const c_char, subtitle: *const c_char) -> ShkMenuPage {
    let (t, st) = (s(title).unwrap_or_default(), s(subtitle).unwrap_or_default());
    with(|b| match b {
        Backend::Drawn(m) => m.model.add_page(owner as u64, parent, &t, &st),
        Backend::Mirror(bm) => bm.add_page(parent, &t),
    })
    .unwrap_or(0)
}

unsafe extern "C" fn add_item(owner: *mut ShkPlugin, page: ShkMenuPage, d: *const ShkMenuItemDesc) -> ShkMenuItem {
    if d.is_null() || ((*d).size as usize) < std::mem::size_of::<ShkMenuItemDesc>() {
        return 0;
    }
    let spec = ItemSpec::from_desc(&*d);
    let cb = (*d).on_event.map(|f| (f, (*d).user as usize));
    with(|b| match b {
        Backend::Drawn(m) => m.model.add_item(owner as u64, page, spec, cb),
        Backend::Mirror(bm) => bm.add_item(owner as u64, page, &spec, (*d).on_event, (*d).user),
    })
    .unwrap_or(0)
}

unsafe extern "C" fn set_value(item: ShkMenuItem, v: f64) -> i32 {
    with(|b| match b {
        Backend::Drawn(m) => m.model.set_value(item, v),
        Backend::Mirror(bm) => bm.set_value(item, v),
    })
    .unwrap_or(false) as i32
}

unsafe extern "C" fn set_text(item: ShkMenuItem, label: *const c_char, right: *const c_char, hint: *const c_char) -> i32 {
    let (l, r, h) = (s(label), s(right), s(hint));
    with(|b| match b {
        Backend::Drawn(m) => m.model.set_text(item, l.as_deref(), r.as_deref(), h.as_deref()),
        Backend::Mirror(bm) => bm.set_text(item, l.as_deref(), r.as_deref()),
    })
    .unwrap_or(false) as i32
}

unsafe extern "C" fn set_enabled(item: ShkMenuItem, on: i32, reason: *const c_char) -> i32 {
    let r = s(reason).unwrap_or_default();
    with(|b| match b {
        Backend::Drawn(m) => m.model.set_enabled(item, on != 0, &r),
        Backend::Mirror(_) => false,
    })
    .unwrap_or(false) as i32
}

unsafe extern "C" fn clear_page(page: ShkMenuPage) -> i32 {
    with(|b| match b {
        Backend::Drawn(m) => m.model.clear_page(page),
        Backend::Mirror(_) => false,
    })
    .unwrap_or(false) as i32
}

unsafe extern "C" fn remove_owner(owner: *mut ShkPlugin) -> i32 {
    with(|b| match b {
        Backend::Drawn(m) => {
            m.model.remove_owner(owner as u64);
        }
        Backend::Mirror(bm) => bm.remove_owner(owner as u64),
    })
    .is_some() as i32
}

unsafe extern "C" fn set_hud(owner: *mut ShkPlugin, slot: u32, text: *const c_char) -> i32 {
    let t = s(text).unwrap_or_default();
    with(|b| match b {
        Backend::Drawn(m) => m.model.set_hud(owner as u64, slot, &t),
        Backend::Mirror(bm) => bm.set_hud(owner as u64, slot, &t),
    })
    .unwrap_or(false) as i32
}

unsafe extern "C" fn is_open() -> i32 {
    with(|b| match b {
        Backend::Drawn(m) => m.nav.open,
        Backend::Mirror(_) => false,
    })
    .unwrap_or(false) as i32
}

unsafe extern "C" fn toast(_owner: *mut ShkPlugin, text: *const c_char, seconds: f32) -> i32 {
    let t = s(text).unwrap_or_default();
    // one stack of notices: the host's (it draws them where it can)
    match *NOTIFY.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(f) => f(&t, seconds),
        None => {
            with(|b| {
                if let Backend::Drawn(m) = b {
                    m.model.toast(&t, seconds, now());
                }
            });
        }
    }
    1
}

/// The table published under "scripthook.menu".
pub static SERVICE: ShkMenuService = ShkMenuService {
    size: std::mem::size_of::<ShkMenuService>() as u32,
    version: 1,
    add_page,
    add_item,
    set_value,
    set_text,
    set_enabled,
    clear_page,
    remove_owner,
    set_hud,
    is_open,
    toast,
};

/// Runs events (no lock held: owners may call the service back).
pub fn run(d: Vec<Dispatch>) {
    for x in d {
        // SAFETY: owners remove their items before they unload.
        unsafe { x.run() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scripthook_sdk::menu::with_desc;

    #[test]
    fn the_c_service_on_the_drawn_backend() {
        set_backend(Some(Backend::Drawn(Box::new(MenuCore::new("test")))), None);
        let me = 0x77 as *mut ShkPlugin;
        unsafe {
            let p = (SERVICE.add_page)(me, 0, c"Trainer".as_ptr(), c"sub".as_ptr());
            assert!(p > 0);
            let spec = ItemSpec::choice("Weather", vec!["CLEAR".into(), "RAIN".into()], 1).hint("h");
            let it = with_desc(&spec, None, std::ptr::null_mut(), |d| (SERVICE.add_item)(me, p, d));
            assert!(it > 0);
            assert_eq!((SERVICE.set_enabled)(it, 0, c"needs native X".as_ptr()), 1);
            assert_eq!((SERVICE.set_hud)(me, 0, c"FPS 60".as_ptr()), 1);
            with(|b| {
                let Backend::Drawn(m) = b else { panic!() };
                let i = m.model.item(it).unwrap();
                assert_eq!(i.spec.options, vec!["CLEAR", "RAIN"]);
                assert_eq!(i.spec.value, 1.0);
                assert_eq!(i.spec.reason.as_deref(), Some("needs native X"));
                assert_eq!(m.model.hud.len(), 1);
            });
            assert_eq!((SERVICE.remove_owner)(me), 1);
            with(|b| {
                let Backend::Drawn(m) = b else { panic!() };
                assert!(m.model.item(it).is_none() && m.model.hud.is_empty());
            });
        }
        set_backend(None, None);
    }
}
