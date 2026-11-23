//! Navigation: the page stack, selection and scrolling, number and text entry, filters. Turns
//! [`Nav`] inputs on the rows of the current page into [`Action`]s.

use crate::model::{ItemId, PageId};
use scripthook_sdk::menu::{ItemSpec, Kind};

/// Navigation input (keyboard and gamepad are mapped to these).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    PageUp,
    PageDown,
    Home,
    End,
    /// LB: next adjustment ×0.1
    Fine,
    /// RB: next adjustment ×10
    Coarse,
    Char(char),
    Backspace,
}

/// One row of the current page.
#[derive(Debug, Clone)]
pub struct Row {
    pub item: Option<ItemId>,
    pub spec: ItemSpec,
}

impl Row {
    pub fn selectable(&self) -> bool {
        match self.spec.kind {
            Kind::Text => !self.spec.hint.is_empty() || self.spec.reason.is_some(),
            Kind::Separator => false,
            _ => true,
        }
    }
}

/// What the user did.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Activate(Option<ItemId>),
    Change(Option<ItemId>, f64),
    Text(Option<ItemId>, String),
    /// a sub-page was opened
    Open(PageId),
    Close,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub page: PageId,
    pub sel: usize,
    pub scroll: usize,
    pub filter: String,
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub buf: String,
    pub text: bool,
}

pub struct Navigator {
    pub open: bool,
    pub stack: Vec<Frame>,
    pub edit: Option<Edit>,
    mult: f64,
    /// rows visible at once (set by the renderer)
    pub visible: usize,
}

impl Default for Navigator {
    fn default() -> Self {
        Navigator { open: false, stack: vec![Frame { page: 0, sel: 0, scroll: 0, filter: String::new() }], edit: None, mult: 1.0, visible: 12 }
    }
}

impl Navigator {
    pub fn top(&self) -> &Frame {
        self.stack.last().unwrap()
    }
    fn top_mut(&mut self) -> &mut Frame {
        self.stack.last_mut().unwrap()
    }
    pub fn page(&self) -> PageId {
        self.top().page
    }
    pub fn selected(&self) -> usize {
        self.top().sel
    }
    pub fn filter(&self) -> &str {
        &self.top().filter
    }
    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }
    pub fn set_open(&mut self, on: bool) {
        self.open = on;
        self.edit = None;
        self.mult = 1.0;
    }
    pub fn push(&mut self, page: PageId) {
        self.stack.push(Frame { page, sel: 0, scroll: 0, filter: String::new() });
        self.edit = None;
    }
    pub fn reset(&mut self) {
        self.stack.truncate(1);
        self.edit = None;
    }

    /// Keeps the selection on a selectable row and inside the scroll window.
    pub fn fix(&mut self, rows: &[Row]) {
        let vis = self.visible.max(1);
        let t = self.top_mut();
        if rows.is_empty() {
            t.sel = 0;
            t.scroll = 0;
            return;
        }
        t.sel = t.sel.min(rows.len() - 1);
        if !rows[t.sel].selectable() {
            if let Some(k) = (t.sel..rows.len()).chain((0..t.sel).rev()).find(|&k| rows[k].selectable()) {
                t.sel = k;
            }
        }
        // keep one row of context above / below when possible
        let margin = if vis > 4 { 1 } else { 0 };
        if t.sel < t.scroll + margin {
            t.scroll = t.sel.saturating_sub(margin);
        }
        if t.sel + margin >= t.scroll + vis {
            t.scroll = (t.sel + margin + 1).saturating_sub(vis);
        }
        t.scroll = t.scroll.min(rows.len().saturating_sub(vis));
    }

    fn mv(&mut self, rows: &[Row], d: i64, wrap: bool) {
        let n = rows.len() as i64;
        if n == 0 {
            return;
        }
        let t = self.top_mut();
        let mut k = t.sel as i64;
        for _ in 0..n {
            let next = k + d;
            k = if wrap { next.rem_euclid(n) } else { next.clamp(0, n - 1) };
            if rows[k as usize].selectable() || (!wrap && (k == 0 || k == n - 1)) {
                break;
            }
        }
        t.sel = k as usize;
    }

    /// Applies navigation to the current page's rows.
    pub fn handle(&mut self, navs: &[Nav], rows: &[Row], filterable: bool) -> Vec<Action> {
        let mut out = Vec::new();
        self.fix(rows);
        for &n in navs {
            let sel = self.selected();
            let row = rows.get(sel);
            if let Some(e) = &mut self.edit {
                match n {
                    Nav::Char(c) if e.text && !c.is_control() && e.buf.chars().count() < 64 => e.buf.push(c),
                    Nav::Char(c) if !e.text && (c.is_ascii_digit() || matches!(c, '.' | '-' | 'x' | 'a'..='f' | 'A'..='F')) => e.buf.push(c),
                    Nav::Backspace => {
                        e.buf.pop();
                    }
                    Nav::Accept => {
                        let e = self.edit.take().unwrap();
                        if let Some(r) = row {
                            if e.text {
                                out.push(Action::Text(r.item, e.buf));
                            } else if let Some(v) = parse_number(&e.buf) {
                                let v = if r.spec.kind == Kind::Slider { v.clamp(r.spec.min, r.spec.max) } else { v };
                                out.push(Action::Change(r.item, v));
                            }
                        }
                    }
                    Nav::Back => self.edit = None,
                    _ => {}
                }
                continue;
            }
            match n {
                Nav::Up => self.mv(rows, -1, true),
                Nav::Down => self.mv(rows, 1, true),
                Nav::PageUp => self.mv(rows, -(self.visible.max(2) as i64 - 1), false),
                Nav::PageDown => self.mv(rows, self.visible.max(2) as i64 - 1, false),
                Nav::Home => {
                    self.top_mut().sel = 0;
                    self.mv(rows, 0, false);
                }
                Nav::End => self.top_mut().sel = rows.len().saturating_sub(1),
                Nav::Fine => self.mult = 0.1,
                Nav::Coarse => self.mult = 10.0,
                Nav::Char(c) if filterable && !c.is_control() => {
                    self.top_mut().filter.push(c);
                    self.top_mut().sel = 0;
                }
                Nav::Backspace if filterable && !self.top().filter.is_empty() => {
                    self.top_mut().filter.pop();
                }
                Nav::Back | Nav::Backspace => {
                    if !self.top().filter.is_empty() && n == Nav::Back {
                        self.top_mut().filter.clear();
                    } else if self.stack.len() > 1 {
                        self.stack.pop();
                    } else {
                        self.set_open(false);
                        out.push(Action::Close);
                    }
                }
                Nav::Accept | Nav::Left | Nav::Right => {
                    let Some(r) = row else { continue };
                    if r.spec.reason.is_some() {
                        continue;
                    }
                    let s = &r.spec;
                    let dir = if n == Nav::Left { -1.0 } else { 1.0 };
                    match (s.kind, n) {
                        (Kind::Subpage, Nav::Accept | Nav::Right) => {
                            out.push(Action::Open(s.target));
                            self.push(s.target);
                        }
                        (Kind::Button, Nav::Accept) => out.push(Action::Activate(r.item)),
                        (Kind::Toggle, Nav::Accept) => out.push(Action::Change(r.item, if s.value != 0.0 { 0.0 } else { 1.0 })),
                        (Kind::Toggle, _) => out.push(Action::Change(r.item, if n == Nav::Right { 1.0 } else { 0.0 })),
                        (Kind::Slider, Nav::Left | Nav::Right) => {
                            let v = (s.value + dir * s.step * self.mult).clamp(s.min, s.max);
                            out.push(Action::Change(r.item, snap(v, s.min, s.step)));
                        }
                        (Kind::Number, Nav::Left | Nav::Right) => out.push(Action::Change(r.item, s.value + dir * s.step.max(f64::MIN_POSITIVE) * self.mult)),
                        (Kind::Slider | Kind::Number, Nav::Accept) => self.edit = Some(Edit { buf: trim_num(s.value), text: false }),
                        (Kind::Choice, _) if !s.options.is_empty() => {
                            let n_ = s.options.len() as i64;
                            let k = (s.value as i64 + if n == Nav::Left { -1 } else { 1 }).rem_euclid(n_);
                            out.push(Action::Change(r.item, k as f64));
                        }
                        (Kind::Input, Nav::Accept) => self.edit = Some(Edit { buf: s.text.clone(), text: true }),
                        _ => {}
                    }
                    if n != Nav::Accept {
                        self.mult = 1.0;
                    }
                }
                _ => {}
            }
            self.fix(rows);
        }
        out
    }
}

fn snap(v: f64, min: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return v;
    }
    let k = ((v - min) / step).round();
    let r = min + k * step;
    // keep sub-step precision readable
    (r * 1e6).round() / 1e6
}

/// A number as typed: decimal, exponent, or `0x` hex.
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).ok().map(|v| v as f64);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Rows whose label contains `filter` (case-insensitive); separators and text are dropped while
/// filtering.
pub fn filtered(rows: Vec<Row>, filter: &str) -> Vec<Row> {
    if filter.is_empty() {
        return rows;
    }
    let f = filter.to_ascii_lowercase();
    rows.into_iter().filter(|r| r.spec.kind == Kind::Input || (r.selectable() && r.spec.label.to_ascii_lowercase().contains(&f))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(item: u32, spec: ItemSpec) -> Row {
        Row { item: Some(item), spec }
    }

    #[test]
    fn navigation_and_actions() {
        let rows = vec![
            row(1, ItemSpec::separator("Player")),
            row(2, ItemSpec::toggle("God mode", false)),
            row(3, ItemSpec::toggle("Money", false).disabled("location unknown")),
            row(4, ItemSpec::slider("Speed", 1.0, 0.0, 5.0, 0.25)),
            row(5, ItemSpec::choice("Weather", vec!["CLEAR".into(), "RAIN".into()], 0)),
            row(6, ItemSpec::input("Model", "ad")),
            row(7, ItemSpec::subpage("Lists", 42)),
        ];
        let mut n = Navigator::default();
        n.set_open(true);
        assert!(n.handle(&[], &rows, false).is_empty());
        assert_eq!(n.selected(), 1, "separator skipped");
        assert_eq!(n.handle(&[Nav::Accept], &rows, false), vec![Action::Change(Some(2), 1.0)]);
        // disabled: selectable, no action
        assert!(n.handle(&[Nav::Down, Nav::Accept], &rows, false).is_empty());
        assert_eq!(n.selected(), 2);
        assert_eq!(n.handle(&[Nav::Down, Nav::Right], &rows, false), vec![Action::Change(Some(4), 1.25)]);
        assert_eq!(n.handle(&[Nav::Coarse, Nav::Right], &rows, false), vec![Action::Change(Some(4), 3.5)]);
        // number entry on the slider
        let a = n.handle(&[Nav::Accept, Nav::Backspace, Nav::Char('9'), Nav::Accept], &rows, false);
        assert_eq!(a, vec![Action::Change(Some(4), 5.0)], "clamped to max");
        assert_eq!(n.handle(&[Nav::Down, Nav::Left], &rows, false), vec![Action::Change(Some(5), 1.0)]);
        let a = n.handle(&[Nav::Down, Nav::Accept, Nav::Char('d'), Nav::Char('e'), Nav::Char('r'), Nav::Accept], &rows, false);
        assert_eq!(a, vec![Action::Text(Some(6), "adder".into())]);
        assert_eq!(n.handle(&[Nav::Down, Nav::Accept], &rows, false), vec![Action::Open(42)]);
        assert_eq!(n.page(), 42);
        n.handle(&[Nav::Back], &[], false);
        assert_eq!(n.page(), 0);
        assert_eq!(n.handle(&[Nav::Back], &rows, false), vec![Action::Close]);
        assert!(!n.open);
        // wrap-around skips the separator
        let mut n = Navigator::default();
        n.handle(&[Nav::Up], &rows, false);
        assert_eq!(n.selected(), 6);
        n.handle(&[Nav::Down], &rows, false);
        assert_eq!(n.selected(), 1);
    }

    #[test]
    fn scrolling_and_filter() {
        let rows: Vec<Row> = (0..50).map(|i| row(i, ItemSpec::button(format!("Vehicle {i}")))).collect();
        let mut n = Navigator { visible: 10, ..Default::default() };
        n.handle(&[Nav::PageDown, Nav::PageDown], &rows, true);
        assert_eq!(n.selected(), 18);
        assert!(n.top().scroll <= 18 && 18 < n.top().scroll + 10);
        n.handle(&[Nav::End], &rows, true);
        assert_eq!(n.top().scroll, 40);
        n.handle(&[Nav::Char('4'), Nav::Char('2')], &rows, true);
        assert_eq!(n.filter(), "42");
        assert_eq!(filtered(rows.clone(), n.filter()).len(), 1);
        n.handle(&[Nav::Back], &rows, true);
        assert_eq!(n.filter(), "");
        assert_eq!(trim_num(2.5), "2.5");
        assert_eq!(parse_number("0x10"), Some(16.0));
    }
}
