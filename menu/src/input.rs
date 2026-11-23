//! Keyboard and gamepad → [`Nav`], with auto-repeat for held directions. Works from key events
//! (hosts with `input->on_key`) or by polling held keys (`input->key_down`) each frame.

use crate::nav::Nav;
use scripthook_sdk::ffi::*;
use scripthook_sdk::keys::*;

/// Keys and buttons the menu listens to.
#[derive(Debug, Clone)]
pub struct Bindings {
    pub open_key: u32,
    pub open_chord: u32,
    pub numpad: bool,
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings { open_key: 0x73, open_chord: SHK_PAD_RB | SHK_PAD_LEFT, numpad: true }
    }
}

const REPEAT_DELAY: f64 = 0.35;
const REPEAT_EVERY: f64 = 0.06;

/// Output of one frame of input.
#[derive(Debug, Default, PartialEq)]
pub struct Frame {
    pub toggle: bool,
    pub navs: Vec<Nav>,
}

#[derive(Default)]
pub struct Input {
    pub bind: Bindings,
    /// held state of the polled keys last frame
    held_keys: Vec<(u32, bool, f64)>,
    pad_prev: u32,
    pad_since: f64,
    pad_next: f64,
}

/// Keys polled when the host gives no key events.
const POLLED: [u32; 16] = [VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_RETURN, VK_ESCAPE, VK_BACK, VK_PRIOR, VK_NEXT, VK_HOME, VK_END, 0x68, 0x62, 0x64, 0x66, 0x65];

fn key_nav(vk: u32, numpad: bool, editing: bool) -> Option<Nav> {
    Some(match vk {
        VK_UP => Nav::Up,
        VK_DOWN => Nav::Down,
        VK_LEFT => Nav::Left,
        VK_RIGHT => Nav::Right,
        VK_RETURN => Nav::Accept,
        VK_ESCAPE => Nav::Back,
        VK_BACK => Nav::Backspace,
        VK_PRIOR => Nav::PageUp,
        VK_NEXT => Nav::PageDown,
        VK_HOME => Nav::Home,
        VK_END => Nav::End,
        0x68 if numpad && !editing => Nav::Up,
        0x62 if numpad && !editing => Nav::Down,
        0x64 if numpad && !editing => Nav::Left,
        0x66 if numpad && !editing => Nav::Right,
        0x65 if numpad && !editing => Nav::Accept,
        0x60 if numpad && !editing => Nav::Back,
        _ => return None,
    })
}

fn repeats(vk: u32) -> bool {
    matches!(vk, VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT | VK_PRIOR | VK_NEXT | VK_BACK | 0x68 | 0x62 | 0x64 | 0x66)
}

impl Input {
    pub fn new(bind: Bindings) -> Input {
        Input { bind, ..Default::default() }
    }

    /// A key event (hosts with on_key). Returns true when the menu consumes it.
    pub fn key_event(&mut self, ev: &ShkKeyEvent, open: bool, editing: bool, out: &mut Frame) -> bool {
        if ev.down == 0 {
            return open;
        }
        if ev.vk == self.bind.open_key && ev.repeat == 0 && !editing {
            out.toggle = true;
            return true;
        }
        if !open {
            return false;
        }
        if ev.ch >= 32 && ev.vk == 0 {
            if let Some(c) = char::from_u32(ev.ch) {
                out.navs.push(Nav::Char(c));
            }
            return true;
        }
        if let Some(n) = key_nav(ev.vk, self.bind.numpad, editing) {
            if ev.repeat == 0 || repeats(ev.vk) {
                out.navs.push(n);
            }
        }
        true
    }

    /// Polls held keys (hosts without key events): edges and auto-repeat. Typed characters are
    /// approximated from letter / digit keys.
    pub fn poll_keys(&mut self, key_down: &dyn Fn(u32) -> bool, open: bool, editing: bool, now: f64, out: &mut Frame) {
        let mut keys: Vec<u32> = POLLED.to_vec();
        keys.push(self.bind.open_key);
        if open {
            keys.extend(0x30..=0x39u32);
            keys.extend(0x41..=0x5Au32);
            keys.extend([VK_SPACE, 0xBD, 0xBE, 0x6D, 0x6E]);
        }
        for vk in keys {
            let down = key_down(vk);
            let st = match self.held_keys.iter_mut().find(|x| x.0 == vk) {
                Some(s) => s,
                None => {
                    self.held_keys.push((vk, false, 0.0));
                    self.held_keys.last_mut().unwrap()
                }
            };
            let was = st.1;
            st.1 = down;
            let fire = if down && !was {
                st.2 = now + REPEAT_DELAY;
                true
            } else if down && repeats(vk) && now >= st.2 {
                st.2 = now + REPEAT_EVERY;
                true
            } else {
                false
            };
            if !fire {
                continue;
            }
            if vk == self.bind.open_key && !editing {
                out.toggle = true;
                continue;
            }
            if !open {
                continue;
            }
            let shift = key_down(VK_SHIFT);
            let ch = match vk {
                0x30..=0x39 if !(0x60..=0x69).contains(&vk) => Some((b'0' + (vk - 0x30) as u8) as char),
                0x41..=0x5A => Some(if shift { vk as u8 as char } else { (vk as u8 as char).to_ascii_lowercase() }),
                VK_SPACE => Some(' '),
                0xBD | 0x6D => Some(if shift { '_' } else { '-' }),
                0xBE | 0x6E => Some('.'),
                _ => None,
            };
            if let Some(c) = ch {
                out.navs.push(Nav::Char(c));
            } else if let Some(n) = key_nav(vk, self.bind.numpad, editing) {
                out.navs.push(n);
            }
        }
    }

    /// The pad: chord opens / closes; D-pad, A, B, LB, RB navigate (auto-repeat on the D-pad).
    pub fn poll_pad(&mut self, pad: Option<ShkPad>, open: bool, now: f64, out: &mut Frame) {
        let held = pad.map(|p| p.buttons).unwrap_or(0);
        let rising = held & !self.pad_prev;
        self.pad_prev = held;
        let chord = self.bind.open_chord;
        if chord != 0 && held & chord == chord && rising & chord != 0 {
            out.toggle = true;
            return;
        }
        if !open {
            return;
        }
        let dirs = SHK_PAD_UP | SHK_PAD_DOWN | SHK_PAD_LEFT | SHK_PAD_RIGHT;
        let mut fire = rising;
        if held & dirs != 0 {
            if rising & dirs != 0 {
                self.pad_since = now;
                self.pad_next = now + REPEAT_DELAY;
            } else if now >= self.pad_next {
                self.pad_next = now + REPEAT_EVERY;
                fire |= held & dirs;
            }
        }
        for (bit, n) in [(SHK_PAD_UP, Nav::Up), (SHK_PAD_DOWN, Nav::Down), (SHK_PAD_LEFT, Nav::Left), (SHK_PAD_RIGHT, Nav::Right), (SHK_PAD_A, Nav::Accept), (SHK_PAD_B, Nav::Back), (SHK_PAD_LB, Nav::Fine), (SHK_PAD_RB, Nav::Coarse)] {
            if fire & bit != 0 {
                out.navs.push(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn polling_edges_repeat_and_chars() {
        let mut i = Input::default();
        let held: RefCell<Vec<u32>> = RefCell::new(vec![]);
        let kd = |vk: u32| held.borrow().contains(&vk);
        let mut f = Frame::default();
        held.borrow_mut().push(0x73);
        i.poll_keys(&kd, false, false, 0.0, &mut f);
        assert!(f.toggle);
        let mut f = Frame::default();
        i.poll_keys(&kd, true, false, 0.1, &mut f);
        assert!(!f.toggle, "held, no repeat of the open key");
        held.borrow_mut().clear();
        held.borrow_mut().push(VK_DOWN);
        let mut f = Frame::default();
        i.poll_keys(&kd, true, false, 1.0, &mut f);
        assert_eq!(f.navs, vec![Nav::Down]);
        let mut f = Frame::default();
        i.poll_keys(&kd, true, false, 1.2, &mut f);
        assert!(f.navs.is_empty(), "within the repeat delay");
        let mut f = Frame::default();
        i.poll_keys(&kd, true, false, 1.4, &mut f);
        assert_eq!(f.navs, vec![Nav::Down], "repeat");
        held.borrow_mut().clear();
        held.borrow_mut().push(0x41);
        let mut f = Frame::default();
        i.poll_keys(&kd, true, false, 2.0, &mut f);
        assert_eq!(f.navs, vec![Nav::Char('a')]);
        // pad chord and buttons
        let mut f = Frame::default();
        i.poll_pad(Some(ShkPad { buttons: SHK_PAD_RB | SHK_PAD_LEFT, ..Default::default() }), false, 3.0, &mut f);
        assert!(f.toggle);
        let mut f = Frame::default();
        i.poll_pad(Some(ShkPad { buttons: SHK_PAD_A, ..Default::default() }), true, 3.1, &mut f);
        assert_eq!(f.navs, vec![Nav::Accept]);
        // key events
        let mut f = Frame::default();
        assert!(i.key_event(&ShkKeyEvent { vk: 0, down: 1, repeat: 0, ch: 'x' as u32 }, true, true, &mut f));
        assert!(!i.key_event(&ShkKeyEvent { vk: VK_DOWN, down: 1, repeat: 0, ch: 0 }, false, false, &mut f));
        assert_eq!(f.navs, vec![Nav::Char('x')]);
    }
}
