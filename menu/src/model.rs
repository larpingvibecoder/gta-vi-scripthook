//! The menu's content: pages and items registered through the menu service (by any plugin), HUD
//! lines and toasts. Pure data; no drawing, no input.

use scripthook_sdk::ffi::ShkMenuEventFn;
use scripthook_sdk::menu::{ItemSpec, Kind};

pub type PageId = u32;
pub type ItemId = u32;

/// The root page: lists the pages registered with parent 0, then the menu's own pages.
pub const ROOT: PageId = 0;

#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub parent: PageId,
    pub owner: u64,
    pub title: String,
    pub subtitle: String,
    pub items: Vec<ItemId>,
    pub filterable: bool,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: ItemId,
    pub owner: u64,
    pub page: PageId,
    pub spec: ItemSpec,
    /// (callback, user) of the owner
    pub cb: Option<(ShkMenuEventFn, usize)>,
}

impl Item {
    pub fn enabled(&self) -> bool {
        self.spec.reason.is_none()
    }
    /// Can the cursor rest on it?
    pub fn selectable(&self) -> bool {
        match self.spec.kind {
            Kind::Text => !self.spec.hint.is_empty() || self.spec.reason.is_some(),
            Kind::Separator => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub until: f64,
    pub start: f64,
}

#[derive(Default)]
pub struct Model {
    pub pages: Vec<Page>,
    pub items: Vec<Item>,
    /// (owner, slot, text)
    pub hud: Vec<(u64, u32, String)>,
    pub toasts: Vec<Toast>,
    next: u32,
}

impl Model {
    pub fn new() -> Model {
        Model { next: 1, ..Default::default() }
    }

    fn id(&mut self) -> u32 {
        let i = self.next.max(1);
        self.next = i + 1;
        i
    }

    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn item(&self, id: ItemId) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn add_page(&mut self, owner: u64, parent: PageId, title: &str, subtitle: &str) -> PageId {
        if parent != ROOT && self.page(parent).is_none() {
            return 0;
        }
        let id = self.id();
        self.pages.push(Page { id, parent, owner, title: title.to_string(), subtitle: subtitle.to_string(), items: Vec::new(), filterable: false });
        id
    }

    pub fn add_item(&mut self, owner: u64, page: PageId, spec: ItemSpec, cb: Option<(ShkMenuEventFn, usize)>) -> ItemId {
        if self.page(page).is_none() {
            return 0;
        }
        if spec.kind == Kind::Subpage && spec.filterable {
            if let Some(t) = self.pages.iter_mut().find(|p| p.id == spec.target) {
                t.filterable = true;
            }
        }
        let id = self.id();
        self.items.push(Item { id, owner, page, spec, cb });
        if let Some(p) = self.pages.iter_mut().find(|p| p.id == page) {
            p.items.push(id);
        }
        id
    }

    pub fn set_value(&mut self, item: ItemId, v: f64) -> bool {
        match self.item_mut(item) {
            Some(i) => {
                i.spec.value = v;
                true
            }
            None => false,
        }
    }

    pub fn set_text(&mut self, item: ItemId, label: Option<&str>, right: Option<&str>, hint: Option<&str>) -> bool {
        let Some(i) = self.item_mut(item) else { return false };
        if let Some(l) = label {
            i.spec.label = l.to_string();
        }
        if let Some(r) = right {
            i.spec.right = (!r.is_empty()).then(|| r.to_string());
        }
        if let Some(h) = hint {
            i.spec.hint = h.to_string();
        }
        true
    }

    pub fn set_enabled(&mut self, item: ItemId, on: bool, reason: &str) -> bool {
        let Some(i) = self.item_mut(item) else { return false };
        i.spec.reason = if on { None } else { Some(reason.to_string()) };
        true
    }

    pub fn clear_page(&mut self, page: PageId) -> bool {
        let Some(p) = self.pages.iter_mut().find(|p| p.id == page) else { return false };
        let ids = std::mem::take(&mut p.items);
        self.items.retain(|i| !ids.contains(&i.id));
        true
    }

    /// Removes the owner's pages (with their items) and items.
    pub fn remove_owner(&mut self, owner: u64) -> usize {
        let mut gone: Vec<PageId> = self.pages.iter().filter(|p| p.owner == owner).map(|p| p.id).collect();
        // sub-pages of removed pages go too
        loop {
            let more: Vec<PageId> = self.pages.iter().filter(|p| gone.contains(&p.parent) && !gone.contains(&p.id)).map(|p| p.id).collect();
            if more.is_empty() {
                break;
            }
            gone.extend(more);
        }
        let before = self.items.len() + self.pages.len();
        self.pages.retain(|p| !gone.contains(&p.id));
        self.items.retain(|i| i.owner != owner && !gone.contains(&i.page));
        let live: Vec<ItemId> = self.items.iter().map(|i| i.id).collect();
        for p in &mut self.pages {
            p.items.retain(|i| live.contains(i));
        }
        self.hud.retain(|h| h.0 != owner);
        before - self.items.len() - self.pages.len()
    }

    pub fn set_hud(&mut self, owner: u64, slot: u32, text: &str) -> bool {
        if slot > 7 {
            return false;
        }
        self.hud.retain(|h| !(h.0 == owner && h.1 == slot));
        if !text.is_empty() {
            self.hud.push((owner, slot, text.to_string()));
            self.hud.sort_by_key(|h| (h.0, h.1));
        }
        true
    }

    pub fn toast(&mut self, text: &str, seconds: f32, now: f64) {
        self.toasts.push(Toast { text: text.to_string(), start: now, until: now + seconds.clamp(0.5, 30.0) as f64 });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn expire(&mut self, now: f64) {
        self.toasts.retain(|t| t.until > now);
    }

    /// Top-level pages (parent 0) in registration order.
    pub fn roots(&self) -> Vec<&Page> {
        self.pages.iter().filter(|p| p.parent == ROOT).collect()
    }

    /// "Trainer / Player"
    pub fn breadcrumb(&self, page: PageId) -> String {
        let mut parts = Vec::new();
        let mut cur = page;
        let mut guard = 0;
        while cur != ROOT && guard < 16 {
            match self.page(cur) {
                Some(p) => {
                    parts.push(p.title.clone());
                    cur = p.parent;
                }
                None => break,
            }
            guard += 1;
        }
        parts.reverse();
        if parts.is_empty() {
            "Home".into()
        } else {
            parts.join(" / ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_items_owners() {
        let mut m = Model::new();
        let t = m.add_page(7, ROOT, "Trainer", "");
        let p = m.add_page(7, t, "Player", "");
        assert_eq!(m.add_page(7, 999, "x", ""), 0);
        let a = m.add_item(7, p, ItemSpec::toggle("God mode", false), None);
        m.add_item(7, t, ItemSpec::subpage("Player", p).filterable(), None);
        assert!(m.page(p).unwrap().filterable);
        let other = m.add_page(8, ROOT, "Other", "");
        m.add_item(8, other, ItemSpec::button("Go"), None);
        assert!(m.set_value(a, 1.0));
        assert_eq!(m.item(a).unwrap().spec.value, 1.0);
        assert!(m.set_enabled(a, false, "needs native X"));
        assert!(!m.item(a).unwrap().enabled());
        assert_eq!(m.breadcrumb(p), "Trainer / Player");
        m.set_hud(7, 0, "FPS 60");
        m.set_hud(8, 0, "pos");
        assert_eq!(m.roots().len(), 2);
        m.remove_owner(7);
        assert!(m.page(p).is_none() && m.item(a).is_none());
        assert_eq!(m.roots().len(), 1);
        assert_eq!(m.hud.len(), 1);
        assert!(m.clear_page(other));
        assert!(m.page(other).unwrap().items.is_empty());
    }
}
