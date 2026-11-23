//! Windows virtual-key codes by name ("F7", "NUM5", "ENTER", "A", ...) and pad button names.

use crate::ffi::*;

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21;
pub const VK_NEXT: u32 = 0x22;
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_DELETE: u32 = 0x2E;
pub const VK_NUMPAD0: u32 = 0x60;
pub const VK_F1: u32 = 0x70;
pub const VK_LSHIFT: u32 = 0xA0;
pub const VK_LCONTROL: u32 = 0xA2;

/// `"F7"` → 0x76. Letters and digits, F1–F24, NUM0–NUM9, and the names below.
pub fn vk_from_name(s: &str) -> Option<u32> {
    let u = s.trim().to_ascii_uppercase();
    if u.len() == 1 {
        let c = u.as_bytes()[0];
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(n) = u.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(VK_F1 - 1 + n);
        }
    }
    if let Some(n) = u.strip_prefix("NUM").and_then(|n| n.parse::<u32>().ok()) {
        if n <= 9 {
            return Some(VK_NUMPAD0 + n);
        }
    }
    Some(match u.as_str() {
        "BACKSPACE" | "BACK" => VK_BACK,
        "TAB" => VK_TAB,
        "ENTER" | "RETURN" => VK_RETURN,
        "SHIFT" => VK_SHIFT,
        "CTRL" | "CONTROL" => VK_CONTROL,
        "ALT" => VK_MENU,
        "ESC" | "ESCAPE" => VK_ESCAPE,
        "SPACE" => VK_SPACE,
        "PAGEUP" => VK_PRIOR,
        "PAGEDOWN" => VK_NEXT,
        "END" => VK_END,
        "HOME" => VK_HOME,
        "LEFT" => VK_LEFT,
        "UP" => VK_UP,
        "RIGHT" => VK_RIGHT,
        "DOWN" => VK_DOWN,
        "INSERT" => 0x2D,
        "DELETE" | "DEL" => VK_DELETE,
        "MULTIPLY" => 0x6A,
        "ADD" => 0x6B,
        "SUBTRACT" => 0x6D,
        "DECIMAL" => 0x6E,
        "DIVIDE" => 0x6F,
        _ => return None,
    })
}

/// 0x76 → "F7".
pub fn vk_name(vk: u32) -> String {
    match vk {
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        VK_RETURN => "Enter".into(),
        VK_ESCAPE => "Esc".into(),
        VK_SPACE => "Space".into(),
        VK_BACK => "Backspace".into(),
        VK_LEFT => "Left".into(),
        VK_RIGHT => "Right".into(),
        VK_UP => "Up".into(),
        VK_DOWN => "Down".into(),
        _ => format!("key {vk:#x}"),
    }
}

/// `"A"`, `"RB"`, `"LEFT"`, ... → `SHK_PAD_*`.
pub fn pad_button(s: &str) -> Option<u32> {
    Some(match s.trim().to_ascii_uppercase().as_str() {
        "A" => SHK_PAD_A,
        "B" => SHK_PAD_B,
        "X" => SHK_PAD_X,
        "Y" => SHK_PAD_Y,
        "LB" => SHK_PAD_LB,
        "RB" => SHK_PAD_RB,
        "LS" => SHK_PAD_LS,
        "RS" => SHK_PAD_RS,
        "UP" | "DUP" => SHK_PAD_UP,
        "DOWN" | "DDOWN" => SHK_PAD_DOWN,
        "LEFT" | "DLEFT" => SHK_PAD_LEFT,
        "RIGHT" | "DRIGHT" => SHK_PAD_RIGHT,
        "MENU" | "START" => SHK_PAD_MENU,
        "VIEW" | "BACK" => SHK_PAD_VIEW,
        _ => return None,
    })
}

/// `"RB+LEFT"` → buttons mask.
pub fn pad_chord(s: &str) -> Option<u32> {
    s.split('+').map(pad_button).try_fold(0, |a, b| b.map(|b| a | b))
}

/// `"Ctrl+F7"` → (vk, mods).
pub fn hotkey(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0;
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let (key, pre) = parts.split_last()?;
    for m in pre {
        mods |= match m.to_ascii_uppercase().as_str() {
            "SHIFT" => SHK_MOD_SHIFT,
            "CTRL" | "CONTROL" => SHK_MOD_CTRL,
            "ALT" => SHK_MOD_ALT,
            _ => return None,
        };
    }
    Some((vk_from_name(key)?, mods))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(vk_from_name("f7"), Some(0x76));
        assert_eq!(vk_from_name("num5"), Some(0x65));
        assert_eq!(vk_from_name("enter"), Some(0x0D));
        assert_eq!(vk_name(0x76), "F7");
        assert_eq!(hotkey("Ctrl+F7"), Some((0x76, SHK_MOD_CTRL)));
        assert_eq!(pad_chord("RB+Left"), Some(SHK_PAD_RB | SHK_PAD_LEFT));
        assert_eq!(pad_chord("RB+Nope"), None);
    }
}
