//! Menus: one client for the menu service ("scripthook.menu", the gta-vi-scripthook menu plugin)
//! and for the host's basic menu ([`BasicMenu`]: what a host without the menu plugin offers).
//!
//! ```ignore
//! let menu = Menu::connect("My plugin");
//! let page = menu.add_page(0, "My plugin", "");
//! menu.add_item(page, ItemSpec::toggle("God mode", false), |ev| { /* ev.value */ });
//! ```
//!
//! Strings are copied by both backends. Events arrive on the frame thread.

use crate::ffi::*;
use crate::rt;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type PageId = u32;
pub type ItemId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Button,
    Toggle,
    Slider,
    Number,
    Choice,
    Input,
    Subpage,
    Separator,
}

impl Kind {
    pub fn code(self) -> u32 {
        match self {
            Kind::Text => SHK_ITEM_TEXT,
            Kind::Button => SHK_ITEM_BUTTON,
            Kind::Toggle => SHK_ITEM_TOGGLE,
            Kind::Slider => SHK_ITEM_SLIDER,
            Kind::Number => SHK_ITEM_NUMBER,
            Kind::Choice => SHK_ITEM_CHOICE,
            Kind::Input => SHK_ITEM_INPUT,
            Kind::Subpage => SHK_ITEM_SUBPAGE,
            Kind::Separator => SHK_ITEM_SEPARATOR,
        }
    }
    pub fn from_code(c: u32) -> Kind {
        match c {
            SHK_ITEM_BUTTON => Kind::Button,
            SHK_ITEM_TOGGLE => Kind::Toggle,
            SHK_ITEM_SLIDER => Kind::Slider,
            SHK_ITEM_NUMBER => Kind::Number,
            SHK_ITEM_CHOICE => Kind::Choice,
            SHK_ITEM_INPUT => Kind::Input,
            SHK_ITEM_SUBPAGE => Kind::Subpage,
            SHK_ITEM_SEPARATOR => Kind::Separator,
            _ => Kind::Text,
        }
    }
}

/// An item to add.
#[derive(Debug, Clone)]
pub struct ItemSpec {
    pub kind: Kind,
    pub label: String,
    pub hint: String,
    /// Some = disabled with this reason
    pub reason: Option<String>,
    pub right: Option<String>,
    pub options: Vec<String>,
    pub text: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub target: PageId,
    pub hotkey: u32,
    pub filterable: bool,
}

impl ItemSpec {
    pub fn new(kind: Kind, label: impl Into<String>) -> ItemSpec {
        ItemSpec { kind, label: label.into(), hint: String::new(), reason: None, right: None, options: Vec::new(), text: String::new(), value: 0.0, min: 0.0, max: 1.0, step: 1.0, target: 0, hotkey: 0, filterable: false }
    }
    pub fn text(label: impl Into<String>) -> ItemSpec {
        ItemSpec::new(Kind::Text, label)
    }
    pub fn separator(label: impl Into<String>) -> ItemSpec {
        ItemSpec::new(Kind::Separator, label)
    }
    pub fn button(label: impl Into<String>) -> ItemSpec {
        ItemSpec::new(Kind::Button, label)
    }
    pub fn toggle(label: impl Into<String>, on: bool) -> ItemSpec {
        ItemSpec { value: on as i32 as f64, ..ItemSpec::new(Kind::Toggle, label) }
    }
    pub fn slider(label: impl Into<String>, value: f64, min: f64, max: f64, step: f64) -> ItemSpec {
        ItemSpec { value, min, max, step: if step > 0.0 { step } else { (max - min) / 20.0 }, ..ItemSpec::new(Kind::Slider, label) }
    }
    pub fn number(label: impl Into<String>, value: f64, step: f64) -> ItemSpec {
        ItemSpec { value, min: f64::MIN, max: f64::MAX, step, ..ItemSpec::new(Kind::Number, label) }
    }
    pub fn choice(label: impl Into<String>, options: Vec<String>, index: usize) -> ItemSpec {
        ItemSpec { value: index as f64, max: options.len().saturating_sub(1) as f64, options, ..ItemSpec::new(Kind::Choice, label) }
    }
    pub fn input(label: impl Into<String>, text: impl Into<String>) -> ItemSpec {
        ItemSpec { text: text.into(), ..ItemSpec::new(Kind::Input, label) }
    }
    pub fn subpage(label: impl Into<String>, target: PageId) -> ItemSpec {
        ItemSpec { target, ..ItemSpec::new(Kind::Subpage, label) }
    }
    pub fn hint(mut self, h: impl Into<String>) -> ItemSpec {
        self.hint = h.into();
        self
    }
    pub fn disabled(mut self, reason: impl Into<String>) -> ItemSpec {
        self.reason = Some(reason.into());
        self
    }
    pub fn right(mut self, r: impl Into<String>) -> ItemSpec {
        self.right = Some(r.into());
        self
    }
    pub fn hotkey(mut self, vk: u32) -> ItemSpec {
        self.hotkey = vk;
        self
    }
    pub fn filterable(mut self) -> ItemSpec {
        self.filterable = true;
        self
    }

    /// Reads a C description (strings copied).
    ///
    /// # Safety
    /// `d` must be a valid description.
    pub unsafe fn from_desc(d: &ShkMenuItemDesc) -> ItemSpec {
        let s = |p: *const c_char| if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
        let mut options = Vec::new();
        if !d.options.is_null() {
            for k in 0..d.option_count as usize {
                options.push(s(*d.options.add(k)));
            }
        }
        ItemSpec {
            kind: Kind::from_code(d.kind),
            label: s(d.label),
            hint: s(d.hint),
            reason: (d.flags & SHK_ITEMF_DISABLED != 0).then(|| s(d.reason)),
            right: (!d.right.is_null()).then(|| s(d.right)),
            options,
            text: s(d.text),
            value: d.value,
            min: d.min,
            max: d.max,
            step: d.step,
            target: d.target,
            hotkey: d.hotkey,
            filterable: d.flags & SHK_ITEMF_FILTERABLE != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Activate,
    Change,
    Text,
}

#[derive(Debug, Clone)]
pub struct MenuEvent {
    pub kind: EventKind,
    pub item: ItemId,
    pub value: f64,
    pub text: String,
}

impl MenuEvent {
    /// # Safety
    /// `e` must be a valid event.
    pub unsafe fn from_raw(e: &ShkMenuEvent) -> MenuEvent {
        MenuEvent {
            kind: match e.ty {
                SHK_EVENT_CHANGE => EventKind::Change,
                SHK_EVENT_TEXT => EventKind::Text,
                _ => EventKind::Activate,
            },
            item: e.item,
            value: e.value,
            text: if e.text.is_null() { String::new() } else { CStr::from_ptr(e.text).to_string_lossy().into_owned() },
        }
    }
}

type EventBox = Box<dyn FnMut(&MenuEvent) + Send>;

unsafe extern "C" fn event_tramp(user: *mut c_void, e: *const ShkMenuEvent) {
    if rt::unloading() || e.is_null() {
        return;
    }
    let f = &mut *(user as *mut EventBox);
    let ev = MenuEvent::from_raw(&*e);
    rt::guard("menu event", || f(&ev));
}

fn cs(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// A menu client.
#[derive(Clone)]
pub enum Menu {
    Service(&'static ShkMenuService),
    Basic(Arc<Mutex<BasicMenu>>),
    /// neither is available: every call does nothing
    None,
}

impl Menu {
    /// The menu service when a menu plugin publishes one, else the host's basic menu (pages
    /// titled `"<basic_prefix><title>"`).
    pub fn connect(basic_prefix: &str) -> Menu {
        Menu::service().unwrap_or_else(|| Menu::basic(basic_prefix))
    }

    pub fn service() -> Option<Menu> {
        let p = crate::host().find_service(SHK_MENU_SERVICE)?;
        // SAFETY: the service publishes an ShkMenuService.
        let s = unsafe { &*(p as *const ShkMenuService) };
        (s.size as usize >= std::mem::offset_of!(ShkMenuService, toast) + 8).then_some(Menu::Service(s))
    }

    pub fn basic(prefix: &str) -> Menu {
        match crate::host().basic_menu() {
            Some(api) => Menu::Basic(Arc::new(Mutex::new(BasicMenu::new(api, rt::self_ptr(), prefix)))),
            None => Menu::None,
        }
    }

    pub fn is_service(&self) -> bool {
        matches!(self, Menu::Service(_))
    }

    pub fn add_page(&self, parent: PageId, title: &str, subtitle: &str) -> PageId {
        match self {
            Menu::Service(s) => {
                let (t, st) = (cs(title), cs(subtitle));
                // SAFETY: valid strings, copied by the service.
                unsafe { (s.add_page)(rt::self_ptr(), parent, t.as_ptr(), st.as_ptr()) }
            }
            Menu::Basic(b) => lock(b).add_page(parent, title),
            Menu::None => 0,
        }
    }

    /// Adds an item; `f` gets its events (frame thread).
    pub fn add_item(&self, page: PageId, spec: &ItemSpec, f: impl FnMut(&MenuEvent) + Send + 'static) -> ItemId {
        let b: EventBox = Box::new(f);
        let user = rt::leak(b) as *mut c_void;
        self.add_item_raw(page, spec, Some(event_tramp), user)
    }

    /// Adds an item with a C callback.
    pub fn add_item_raw(&self, page: PageId, spec: &ItemSpec, cb: Option<ShkMenuEventFn>, user: *mut c_void) -> ItemId {
        match self {
            Menu::Service(s) => with_desc(spec, cb, user, |d| {
                // SAFETY: a valid description; strings copied by the service.
                unsafe { (s.add_item)(rt::self_ptr(), page, d) }
            }),
            Menu::Basic(b) => lock(b).add_item(rt::self_ptr() as u64, page, spec, cb, user),
            Menu::None => 0,
        }
    }

    pub fn set_value(&self, item: ItemId, v: f64) -> bool {
        match self {
            // SAFETY: plain values.
            Menu::Service(s) => unsafe { (s.set_value)(item, v) != 0 },
            Menu::Basic(b) => lock(b).set_value(item, v),
            Menu::None => false,
        }
    }

    pub fn set_text(&self, item: ItemId, label: Option<&str>, right: Option<&str>, hint: Option<&str>) -> bool {
        match self {
            Menu::Service(s) => {
                let (l, r, h) = (label.map(cs), right.map(cs), hint.map(cs));
                let p = |x: &Option<CString>| x.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null());
                // SAFETY: valid strings or null.
                unsafe { (s.set_text)(item, p(&l), p(&r), p(&h)) != 0 }
            }
            Menu::Basic(b) => lock(b).set_text(item, label, right),
            Menu::None => false,
        }
    }

    pub fn set_enabled(&self, item: ItemId, on: bool, reason: &str) -> bool {
        match self {
            Menu::Service(s) => {
                let r = cs(reason);
                // SAFETY: valid string.
                unsafe { (s.set_enabled)(item, on as i32, r.as_ptr()) != 0 }
            }
            Menu::Basic(_) | Menu::None => false,
        }
    }

    pub fn clear_page(&self, page: PageId) -> bool {
        match self {
            // SAFETY: plain value.
            Menu::Service(s) => unsafe { (s.clear_page)(page) != 0 },
            _ => false,
        }
    }

    /// Removes everything this plugin added (call at unload).
    pub fn remove_all(&self) {
        match self {
            Menu::Service(_) => {
                // the publisher may have been unloaded first: look it up again
                if let Some(Menu::Service(s)) = Menu::service() {
                    // SAFETY: plain value.
                    unsafe { (s.remove_owner)(rt::self_ptr()) };
                }
            }
            Menu::Basic(b) => lock(b).remove_owner(rt::self_ptr() as u64),
            Menu::None => {}
        }
    }

    pub fn set_hud(&self, slot: u32, text: &str) -> bool {
        match self {
            Menu::Service(s) => {
                let t = cs(text);
                // SAFETY: valid string.
                unsafe { (s.set_hud)(rt::self_ptr(), slot, t.as_ptr()) != 0 }
            }
            Menu::Basic(b) => lock(b).set_hud(rt::self_ptr() as u64, slot, text),
            Menu::None => false,
        }
    }

    pub fn is_open(&self) -> bool {
        match self {
            // SAFETY: no arguments.
            Menu::Service(s) => unsafe { (s.is_open)() != 0 },
            _ => false,
        }
    }

    pub fn toast(&self, text: &str, seconds: f32) {
        match self {
            Menu::Service(s) => {
                let t = cs(text);
                // SAFETY: valid string.
                unsafe { (s.toast)(rt::self_ptr(), t.as_ptr(), seconds) };
            }
            _ => crate::host().notify(text, seconds),
        }
    }
}

fn lock(b: &Arc<Mutex<BasicMenu>>) -> std::sync::MutexGuard<'_, BasicMenu> {
    b.lock().unwrap_or_else(|e| e.into_inner())
}

/// Builds the C description of `spec` (valid during `f`).
pub fn with_desc<R>(spec: &ItemSpec, cb: Option<ShkMenuEventFn>, user: *mut c_void, f: impl FnOnce(&ShkMenuItemDesc) -> R) -> R {
    let label = cs(&spec.label);
    let hint = cs(&spec.hint);
    let reason = spec.reason.as_deref().map(cs);
    let right = spec.right.as_deref().map(cs);
    let text = cs(&spec.text);
    let opts: Vec<CString> = spec.options.iter().map(|o| cs(o)).collect();
    let optp: Vec<*const c_char> = opts.iter().map(|o| o.as_ptr()).collect();
    let d = ShkMenuItemDesc {
        size: std::mem::size_of::<ShkMenuItemDesc>() as u32,
        kind: spec.kind.code(),
        flags: if spec.reason.is_some() { SHK_ITEMF_DISABLED } else { 0 } | if spec.filterable { SHK_ITEMF_FILTERABLE } else { 0 },
        option_count: optp.len() as u32,
        label: label.as_ptr(),
        hint: hint.as_ptr(),
        reason: reason.as_ref().map(|r| r.as_ptr()).unwrap_or(std::ptr::null()),
        right: right.as_ref().map(|r| r.as_ptr()).unwrap_or(std::ptr::null()),
        options: if optp.is_empty() { std::ptr::null() } else { optp.as_ptr() },
        text: text.as_ptr(),
        value: spec.value,
        min: spec.min,
        max: spec.max,
        step: spec.step,
        target: spec.target,
        hotkey: spec.hotkey,
        on_event: cb,
        user,
    };
    f(&d)
}

// ---------------------------------------------------------------------------------------------
// The host's basic menu behind the menu-service model

struct BPage {
    id: PageId,
    host: ShkHostPage,
    path: String,
}

/// One item mirrored into the host's basic menu. Boxed: the host keeps pointers to its storage.
struct BItem {
    id: ItemId,
    owner: u64,
    kind: Kind,
    host_page: ShkHostPage,
    /// host item index (text items: for set_text)
    line: i32,
    /// choice: the text line showing the option
    choice_line: i32,
    toggle: AtomicI32,
    slider: AtomicU32,
    number: AtomicU64,
    options: Vec<String>,
    label: String,
    cb: Option<ShkMenuEventFn>,
    user: usize,
    alive: AtomicBool,
    api: &'static ShkHostMenuApi,
    me: usize,
}

/// The menu-service model on top of the host's basic menu (pages of toggles, sliders, numbers,
/// buttons and text drawn by the host). What it cannot show: nested pages become separate host
/// pages titled "Parent > Child"; disabled items become text with their reason; choices become a
/// slider of the index plus a line with the option; text entry is not available; labels of
/// non-text items, enabled state and page contents cannot change after creation.
pub struct BasicMenu {
    api: &'static ShkHostMenuApi,
    me: *mut ShkPlugin,
    prefix: String,
    pages: Vec<BPage>,
    items: Vec<Box<BItem>>,
    hud: Option<(ShkHostPage, Vec<(u64, u32, i32)>)>,
    next: u32,
}

// SAFETY: the raw pointers are the host's plugin handle and boxed items owned here.
unsafe impl Send for BasicMenu {}

unsafe extern "C" fn basic_changed(user: *mut c_void, _value: *const c_void) {
    let it = &*(user as *const BItem);
    if !it.alive.load(Ordering::Acquire) || rt::unloading() {
        return;
    }
    let v = match it.kind {
        Kind::Toggle => it.toggle.load(Ordering::Relaxed) as f64,
        Kind::Slider | Kind::Choice => f32::from_bits(it.slider.load(Ordering::Relaxed)) as f64,
        Kind::Number => f64::from_bits(it.number.load(Ordering::Relaxed)),
        _ => 0.0,
    };
    let v = if it.kind == Kind::Choice { v.round().clamp(0.0, it.options.len().saturating_sub(1) as f64) } else { v };
    if it.kind == Kind::Choice && it.choice_line >= 0 {
        let t = cs(&format!("    = {}", it.options.get(v as usize).cloned().unwrap_or_default()));
        (it.api.set_text)(it.me as *mut ShkPlugin, it.host_page, it.choice_line, t.as_ptr());
    }
    if let Some(cb) = it.cb {
        let ev = ShkMenuEvent { size: std::mem::size_of::<ShkMenuEvent>() as u32, ty: SHK_EVENT_CHANGE, item: it.id, reserved: 0, value: v, text: std::ptr::null() };
        cb(it.user as *mut c_void, &ev);
    }
}

unsafe extern "C" fn basic_pressed(user: *mut c_void) {
    let it = &*(user as *const BItem);
    if !it.alive.load(Ordering::Acquire) || rt::unloading() {
        return;
    }
    if let Some(cb) = it.cb {
        let ev = ShkMenuEvent { size: std::mem::size_of::<ShkMenuEvent>() as u32, ty: SHK_EVENT_ACTIVATE, item: it.id, reserved: 0, value: 0.0, text: std::ptr::null() };
        cb(it.user as *mut c_void, &ev);
    }
}

impl BasicMenu {
    pub fn new(api: &'static ShkHostMenuApi, me: *mut ShkPlugin, prefix: &str) -> BasicMenu {
        BasicMenu { api, me, prefix: prefix.to_string(), pages: Vec::new(), items: Vec::new(), hud: None, next: 1 }
    }

    pub fn add_page(&mut self, parent: PageId, title: &str) -> PageId {
        let path = match self.pages.iter().find(|p| p.id == parent) {
            Some(p) => format!("{} > {title}", p.path),
            None => format!("{}{title}", self.prefix),
        };
        let t = cs(&path);
        // SAFETY: valid pointers.
        let host = unsafe { (self.api.page)(self.me, t.as_ptr()) };
        if host == 0 {
            return 0;
        }
        let id = self.next;
        self.next += 1;
        self.pages.push(BPage { id, host, path });
        id
    }

    /// Host page title of a page (for tests and automation).
    pub fn page_title(&self, page: PageId) -> Option<String> {
        self.pages.iter().find(|p| p.id == page).map(|p| p.path.clone())
    }

    fn text_line(&self, host: ShkHostPage, t: &str) -> i32 {
        let c = cs(t);
        // SAFETY: valid pointers.
        unsafe { (self.api.text)(self.me, host, c.as_ptr()) }
    }

    pub fn add_item(&mut self, owner: u64, page: PageId, spec: &ItemSpec, cb: Option<ShkMenuEventFn>, user: *mut c_void) -> ItemId {
        let Some(host) = self.pages.iter().find(|p| p.id == page).map(|p| p.host) else { return 0 };
        let id = self.next;
        self.next += 1;
        let mut it = Box::new(BItem {
            id,
            owner,
            kind: spec.kind,
            host_page: host,
            line: -1,
            choice_line: -1,
            toggle: AtomicI32::new((spec.value != 0.0) as i32),
            slider: AtomicU32::new((spec.value as f32).to_bits()),
            number: AtomicU64::new(spec.value.to_bits()),
            options: spec.options.clone(),
            label: spec.label.clone(),
            cb,
            user: user as usize,
            alive: AtomicBool::new(true),
            api: self.api,
            me: self.me as usize,
        });
        let me = self.me;
        let api = self.api;
        let label = cs(&spec.label);
        let up = &mut *it as *mut BItem as *mut c_void;
        // SAFETY: the item is boxed and kept for the process lifetime (never dropped while the
        // host may call back).
        unsafe {
            if let Some(r) = &spec.reason {
                it.line = self.text_line(host, &format!("x {} - {r}", spec.label));
            } else {
                it.line = match spec.kind {
                    Kind::Text => self.text_line(host, &match &spec.right {
                        Some(r) => format!("{}  {r}", spec.label),
                        None => spec.label.clone(),
                    }),
                    Kind::Separator => self.text_line(host, &format!("-- {} --", spec.label)),
                    Kind::Button => (api.button)(me, host, label.as_ptr(), Some(basic_pressed), up),
                    Kind::Toggle => (api.toggle)(me, host, label.as_ptr(), it.toggle.as_ptr(), Some(basic_changed), up),
                    Kind::Slider => (api.slider)(me, host, label.as_ptr(), it.slider.as_ptr() as *mut f32, spec.min as f32, spec.max as f32, spec.step as f32, Some(basic_changed), up),
                    Kind::Number => (api.number)(me, host, label.as_ptr(), it.number.as_ptr() as *mut f64, Some(basic_changed), up),
                    Kind::Choice => {
                        let max = spec.options.len().saturating_sub(1) as f32;
                        let l = (api.slider)(me, host, label.as_ptr(), it.slider.as_ptr() as *mut f32, 0.0, max, 1.0, Some(basic_changed), up);
                        it.choice_line = self.text_line(host, &format!("    = {}", spec.options.get(spec.value as usize).cloned().unwrap_or_default()));
                        l
                    }
                    Kind::Input => self.text_line(host, &format!("{}: (typing needs the scripthook menu with drawing)", spec.label)),
                    Kind::Subpage => {
                        let path = self.pages.iter().find(|p| p.id == spec.target).map(|p| p.path.clone()).unwrap_or_default();
                        self.text_line(host, &format!("> {}  (page \"{path}\")", spec.label))
                    }
                };
            }
        }
        self.items.push(it);
        id
    }

    pub fn set_value(&mut self, item: ItemId, v: f64) -> bool {
        let Some(it) = self.items.iter().find(|i| i.id == item) else { return false };
        match it.kind {
            Kind::Toggle => it.toggle.store((v != 0.0) as i32, Ordering::Relaxed),
            Kind::Slider | Kind::Choice => it.slider.store((v as f32).to_bits(), Ordering::Relaxed),
            Kind::Number => it.number.store(v.to_bits(), Ordering::Relaxed),
            _ => return false,
        }
        if it.kind == Kind::Choice && it.choice_line >= 0 {
            let t = cs(&format!("    = {}", it.options.get(v as usize).cloned().unwrap_or_default()));
            // SAFETY: valid pointers.
            unsafe { (self.api.set_text)(self.me, it.host_page, it.choice_line, t.as_ptr()) };
        }
        true
    }

    pub fn set_text(&mut self, item: ItemId, label: Option<&str>, right: Option<&str>) -> bool {
        let Some(it) = self.items.iter_mut().find(|i| i.id == item) else { return false };
        if it.kind != Kind::Text || it.line < 0 {
            return false;
        }
        if let Some(l) = label {
            it.label = l.to_string();
        }
        let t = match right {
            Some(r) if !r.is_empty() => format!("{}  {r}", it.label),
            _ => it.label.clone(),
        };
        let c = cs(&t);
        // SAFETY: valid pointers.
        unsafe { (self.api.set_text)(self.me, it.host_page, it.line, c.as_ptr()) != 0 }
    }

    /// The host cannot remove items: the owner's items stop calling back.
    pub fn remove_owner(&mut self, owner: u64) {
        for it in &self.items {
            if it.owner == owner {
                it.alive.store(false, Ordering::Release);
            }
        }
        if let Some((page, lines)) = &mut self.hud {
            for (o, _, l) in lines.iter_mut() {
                if *o == owner {
                    let c = cs("");
                    // SAFETY: valid pointers.
                    unsafe { (self.api.set_text)(self.me, *page, *l, c.as_ptr()) };
                    *o = 0;
                }
            }
        }
    }

    pub fn set_hud(&mut self, owner: u64, slot: u32, text: &str) -> bool {
        if self.hud.is_none() {
            let t = cs(&format!("{}HUD", self.prefix));
            // SAFETY: valid pointers.
            let p = unsafe { (self.api.page)(self.me, t.as_ptr()) };
            if p == 0 {
                return false;
            }
            self.hud = Some((p, Vec::new()));
        }
        let (api, me) = (self.api, self.me);
        let (page, lines) = self.hud.as_mut().unwrap();
        let line = match lines.iter().find(|x| x.0 == owner && x.1 == slot) {
            Some(x) => x.2,
            None => {
                if text.is_empty() {
                    return true;
                }
                let c = cs(text);
                // SAFETY: valid pointers.
                let l = unsafe { (api.text)(me, *page, c.as_ptr()) };
                lines.push((owner, slot, l));
                return l >= 0;
            }
        };
        let c = cs(text);
        // SAFETY: valid pointers.
        unsafe { (api.set_text)(me, *page, line, c.as_ptr()) != 0 }
    }
}

impl Drop for BasicMenu {
    fn drop(&mut self) {
        // the host keeps pointers to the items until it removes the plugin's pages: never free them
        for it in &self.items {
            it.alive.store(false, Ordering::Release);
        }
        std::mem::forget(std::mem::take(&mut self.items));
    }
}
