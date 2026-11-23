//! Memory cheats: locations (`[[location]]`) and cheats (`[[cheat]]`) of `cheats.toml`, resolved
//! and applied through the host's memory API ([`Mem`]).

use crate::script::Value as SValue;
use scripthook_sdk::config::{toml, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VType {
    I32,
    I64,
    F32,
    F64,
    Vec3,
}

impl VType {
    pub fn parse(s: &str) -> Option<VType> {
        Some(match s {
            "i32" | "int" => VType::I32,
            "i64" => VType::I64,
            "f32" | "float" => VType::F32,
            "f64" | "double" => VType::F64,
            "vec3" => VType::Vec3,
            _ => return None,
        })
    }
    pub fn size(self) -> usize {
        match self {
            VType::I32 | VType::F32 => 4,
            VType::I64 | VType::F64 => 8,
            VType::Vec3 => 12,
        }
    }
    pub fn encode(self, v: &SValue) -> Option<Vec<u8>> {
        Some(match self {
            VType::I32 => (v.as_i64()? as i32).to_le_bytes().to_vec(),
            VType::I64 => v.as_i64()?.to_le_bytes().to_vec(),
            VType::F32 => (v.as_f64()? as f32).to_le_bytes().to_vec(),
            VType::F64 => v.as_f64()?.to_le_bytes().to_vec(),
            VType::Vec3 => match v {
                SValue::Vec3(a) => a.iter().flat_map(|x| (*x as f32).to_le_bytes()).collect(),
                _ => return None,
            },
        })
    }
    pub fn decode(self, b: &[u8]) -> SValue {
        let f = |k: usize| f32::from_le_bytes(b[k..k + 4].try_into().unwrap()) as f64;
        match self {
            VType::I32 => SValue::Int(i32::from_le_bytes(b[..4].try_into().unwrap()) as i64),
            VType::I64 => SValue::Int(i64::from_le_bytes(b[..8].try_into().unwrap())),
            VType::F32 => SValue::Float(f(0)),
            VType::F64 => SValue::Float(f64::from_le_bytes(b[..8].try_into().unwrap())),
            VType::Vec3 => SValue::Vec3([f(0), f(4), f(8)]),
        }
    }
}

/// Where a value is.
#[derive(Debug, Clone, PartialEq)]
pub enum Base {
    Unknown(String),
    Address(u64),
    Offset { module: String, offset: i64 },
    Export { module: String, symbol: String, offset: i64 },
    Pattern { module: String, section: Option<String>, pattern: String, nth: u32, rip: Option<(u32, u32)>, offset: i64 },
}

#[derive(Debug, Clone)]
pub struct Location {
    pub id: String,
    pub ty: VType,
    pub base: Base,
    pub chain: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CKind {
    Freeze,
    Set,
    Multiply,
    Patch,
    /// kinds that need the host's generated detours / fly controller (not available to plugins)
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct Cheat {
    pub id: String,
    pub name: String,
    pub group: String,
    pub kind: CKind,
    pub kind_name: String,
    pub location: Option<Location>,
    pub value: f64,
    pub factor: f64,
    /// -1 decrease only, +1 increase only, 0 both
    pub direction: i32,
    pub bytes: Vec<u8>,
    pub slider: Option<(f64, f64, f64)>,
    pub enabled: bool,
    pub hotkey: Option<(u32, u32)>,
    pub note: String,
}

/// Memory access (the host, or a test double).
pub trait Mem {
    fn module(&self, name: &str) -> Option<u64>;
    fn export(&self, module: &str, symbol: &str) -> Option<u64>;
    fn scan(&self, module: &str, section: Option<&str>, pattern: &str, nth: u32) -> Option<u64>;
    fn rip(&self, insn: u64, disp: u32, len: u32) -> Option<u64>;
    fn chain(&self, base: u64, offsets: &[i64]) -> Option<u64>;
    fn read(&self, addr: u64, out: &mut [u8]) -> bool;
    fn write(&self, addr: u64, data: &[u8]) -> bool;
    fn patch(&self, addr: u64, data: &[u8]) -> bool;
}

pub struct HostMem;

impl Mem for HostMem {
    fn module(&self, name: &str) -> Option<u64> {
        scripthook_sdk::host().module(name).map(|m| m.base)
    }
    fn export(&self, module: &str, symbol: &str) -> Option<u64> {
        scripthook_sdk::host().find_export(module, symbol)
    }
    fn scan(&self, module: &str, section: Option<&str>, pattern: &str, nth: u32) -> Option<u64> {
        scripthook_sdk::host().scan(module, section, pattern, nth)
    }
    fn rip(&self, insn: u64, disp: u32, len: u32) -> Option<u64> {
        scripthook_sdk::host().rip_target(insn, disp, len)
    }
    fn chain(&self, base: u64, offsets: &[i64]) -> Option<u64> {
        scripthook_sdk::host().resolve_chain(base, offsets)
    }
    fn read(&self, addr: u64, out: &mut [u8]) -> bool {
        scripthook_sdk::host().read_bytes(addr, out)
    }
    fn write(&self, addr: u64, data: &[u8]) -> bool {
        scripthook_sdk::host().write_bytes(addr, data)
    }
    fn patch(&self, addr: u64, data: &[u8]) -> bool {
        scripthook_sdk::host().patch(addr, data)
    }
}

fn parse_location(v: &Value, id: &str, ty_default: Option<VType>) -> Result<Location, String> {
    let ty = match v.str_of("type") {
        Some(t) => VType::parse(&t).ok_or_else(|| format!("{id}: type '{t}'"))?,
        None => ty_default.unwrap_or(VType::I32),
    };
    let offset = v.i64_of("offset").unwrap_or(0);
    let module = v.str_of("module").unwrap_or_default();
    let base = if let Some(u) = v.str_of("unknown") {
        Base::Unknown(u)
    } else if let Some(a) = v.i64_of("address") {
        Base::Address(a as u64)
    } else if let Some(s) = v.str_of("export") {
        Base::Export { module, symbol: s, offset }
    } else if let Some(p) = v.str_of("pattern") {
        let rip = v.get("rip").map(|r| (r.i64_of("disp").unwrap_or(3) as u32, r.i64_of("len").unwrap_or(7) as u32));
        Base::Pattern { module, section: v.str_of("section"), pattern: p, nth: v.i64_of("nth").unwrap_or(0) as u32, rip, offset }
    } else if v.get("module").is_some() {
        Base::Offset { module, offset }
    } else {
        Base::Unknown(format!("{id}: no location given"))
    };
    let chain = v.get("chain").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    Ok(Location { id: id.to_string(), ty, base, chain })
}

impl Location {
    /// The address, or why it is not known.
    pub fn resolve(&self, m: &dyn Mem) -> Result<u64, String> {
        let a = match &self.base {
            Base::Unknown(why) => return Err(why.clone()),
            Base::Address(a) => *a,
            Base::Offset { module, offset } => (m.module(module).ok_or_else(|| format!("{}: module {module} not loaded", self.id))? as i64 + offset) as u64,
            Base::Export { module, symbol, offset } => (m.export(module, symbol).ok_or_else(|| format!("{}: no export {symbol}", self.id))? as i64 + offset) as u64,
            Base::Pattern { module, section, pattern, nth, rip, offset } => {
                let mut a = m.scan(module, section.as_deref(), pattern, *nth).ok_or_else(|| format!("{}: pattern not found", self.id))?;
                if let Some((d, l)) = rip {
                    a = m.rip(a, *d, *l).ok_or_else(|| format!("{}: RIP target unreadable", self.id))?;
                }
                (a as i64 + offset) as u64
            }
        };
        if self.chain.is_empty() {
            Ok(a)
        } else {
            m.chain(a, &self.chain).ok_or_else(|| format!("{}: pointer chain broken", self.id))
        }
    }

    pub fn known(&self) -> Result<(), String> {
        match &self.base {
            Base::Unknown(w) => Err(w.clone()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CheatDefs {
    pub locations: Vec<Location>,
    pub cheats: Vec<Cheat>,
    pub errors: Vec<String>,
}

impl CheatDefs {
    pub fn add(&mut self, text: &str, source: &str) {
        let v = match toml::parse(text) {
            Ok(v) => v,
            Err(e) => {
                self.errors.push(format!("{source}: {e}"));
                return;
            }
        };
        for l in v.get("location").and_then(Value::as_array).unwrap_or(&[]) {
            let Some(id) = l.str_of("id") else { continue };
            match parse_location(l, &id, None) {
                Ok(loc) => {
                    self.locations.retain(|x| x.id != id);
                    self.locations.push(loc);
                }
                Err(e) => self.errors.push(format!("{source}: {e}")),
            }
        }
        for c in v.get("cheat").and_then(Value::as_array).unwrap_or(&[]) {
            let Some(id) = c.str_of("id") else { continue };
            let kind_name = c.str_of("kind").unwrap_or_else(|| "freeze".into());
            let kind = match kind_name.as_str() {
                "freeze" => CKind::Freeze,
                "set" => CKind::Set,
                "multiply" => CKind::Multiply,
                "patch" => CKind::Patch,
                _ => CKind::Unsupported,
            };
            let ty = c.str_of("type").and_then(|t| VType::parse(&t));
            let location = match c.get("location") {
                Some(Value::Str(n)) => match self.locations.iter().find(|l| l.id == *n) {
                    Some(l) => Some(Location { ty: ty.unwrap_or(l.ty), ..l.clone() }),
                    None => {
                        self.errors.push(format!("{source}: {id}: no location '{n}'"));
                        None
                    }
                },
                Some(t @ Value::Table(_)) => match parse_location(t, &id, ty) {
                    Ok(l) => Some(l),
                    Err(e) => {
                        self.errors.push(format!("{source}: {e}"));
                        None
                    }
                },
                _ => None,
            };
            let bytes = c.str_of("bytes").map(|b| b.split_whitespace().filter_map(|x| u8::from_str_radix(x, 16).ok()).collect()).unwrap_or_default();
            let cheat = Cheat {
                name: c.str_of("name").unwrap_or_else(|| id.clone()),
                group: c.str_of("group").unwrap_or_else(|| "Misc".into()),
                kind,
                kind_name,
                location,
                value: c.f64_of("value").unwrap_or(0.0),
                factor: c.f64_of("factor").unwrap_or(1.0),
                direction: match c.str_of("direction").as_deref() {
                    Some("decrease") => -1,
                    Some("increase") => 1,
                    _ => 0,
                },
                bytes,
                slider: c.get("slider").map(|s| (s.f64_of("min").unwrap_or(0.0), s.f64_of("max").unwrap_or(1.0), s.f64_of("step").unwrap_or(1.0))),
                enabled: c.bool_of("enabled").unwrap_or(false),
                hotkey: c.str_of("hotkey").and_then(|h| scripthook_sdk::keys::hotkey(&h)),
                note: c.str_of("note").unwrap_or_default(),
                id: id.clone(),
            };
            match self.cheats.iter_mut().find(|x| x.id == id) {
                Some(x) => *x = cheat,
                None => self.cheats.push(cheat),
            }
        }
    }

    pub fn location(&self, id: &str) -> Option<&Location> {
        self.locations.iter().find(|l| l.id == id)
    }
}

impl Cheat {
    /// Why the cheat cannot be used (None = usable).
    pub fn unusable(&self) -> Option<String> {
        if self.kind == CKind::Unsupported {
            return Some(format!("memory cheat kind '{}' needs the host's generated detours", self.kind_name));
        }
        match &self.location {
            None => Some("memory cheat: no location".into()),
            Some(l) => l.known().err().map(|w| format!("memory cheat: {w}")),
        }
    }
}

/// Per-cheat run state.
#[derive(Debug, Clone, Default)]
pub struct CheatState {
    pub on: bool,
    pub value: f64,
    pub addr: Option<u64>,
    pub prev: Option<Vec<u8>>,
    pub saved: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// Applies one cheat for one frame (or its switch).
pub fn apply(c: &Cheat, st: &mut CheatState, m: &dyn Mem, switching: Option<bool>) {
    let Some(loc) = &c.location else { return };
    if st.addr.is_none() {
        match loc.resolve(m) {
            Ok(a) => st.addr = Some(a),
            Err(e) => {
                st.error = Some(e);
                return;
            }
        }
    }
    let a = st.addr.unwrap();
    let ty = loc.ty;
    match (c.kind, switching) {
        (CKind::Patch, Some(true)) => {
            let mut old = vec![0u8; c.bytes.len()];
            if m.read(a, &mut old) && m.patch(a, &c.bytes) {
                st.saved = Some(old);
            } else {
                st.error = Some(format!("{}: patch failed at {a:#x}", c.id));
            }
        }
        (CKind::Patch, Some(false)) => {
            if let Some(old) = st.saved.take() {
                m.patch(a, &old);
            }
        }
        (CKind::Set, Some(true)) | (CKind::Freeze, _) if st.on || switching == Some(true) => {
            let v = if c.kind == CKind::Freeze || c.kind == CKind::Set { st.value } else { c.value };
            if let Some(b) = ty.encode(&SValue::Float(v)) {
                if !m.write(a, &b) {
                    // a pointer chain may have moved: resolve again next frame
                    st.addr = None;
                }
            }
        }
        (CKind::Multiply, None) if st.on => {
            let mut cur = vec![0u8; ty.size()];
            if !m.read(a, &mut cur) {
                st.addr = None;
                return;
            }
            if let Some(prev) = &st.prev {
                if *prev != cur {
                    let (p, c_) = (ty.decode(prev).as_f64().unwrap_or(0.0), ty.decode(&cur).as_f64().unwrap_or(0.0));
                    let d = c_ - p;
                    if (c.direction == 0) || (c.direction < 0 && d < 0.0) || (c.direction > 0 && d > 0.0) {
                        let nv = p + d * st.value;
                        if let Some(b) = ty.encode(&SValue::Float(nv)) {
                            m.write(a, &b);
                            st.prev = Some(b);
                            return;
                        }
                    }
                }
            }
            st.prev = Some(cur);
        }
        (CKind::Multiply, Some(_)) => st.prev = None,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// 64 bytes of fake memory at 0x1000, a module "game.exe" at 0x1000, export "g" at 0x1010.
    struct Fake(RefCell<Vec<u8>>);

    impl Mem for Fake {
        fn module(&self, n: &str) -> Option<u64> {
            (n == "game.exe" || n.is_empty()).then_some(0x1000)
        }
        fn export(&self, _m: &str, s: &str) -> Option<u64> {
            (s == "g").then_some(0x1010)
        }
        fn scan(&self, _m: &str, _s: Option<&str>, p: &str, _n: u32) -> Option<u64> {
            (p == "AA BB").then_some(0x1020)
        }
        fn rip(&self, i: u64, _d: u32, l: u32) -> Option<u64> {
            Some(i + l as u64 + 4)
        }
        fn chain(&self, b: u64, o: &[i64]) -> Option<u64> {
            Some((b as i64 + o.iter().sum::<i64>()) as u64)
        }
        fn read(&self, a: u64, out: &mut [u8]) -> bool {
            let k = (a - 0x1000) as usize;
            out.copy_from_slice(&self.0.borrow()[k..k + out.len()]);
            true
        }
        fn write(&self, a: u64, d: &[u8]) -> bool {
            let k = (a - 0x1000) as usize;
            self.0.borrow_mut()[k..k + d.len()].copy_from_slice(d);
            true
        }
        fn patch(&self, a: u64, d: &[u8]) -> bool {
            self.write(a, d)
        }
    }

    #[test]
    fn shipped_cheats_are_unknown() {
        let mut d = CheatDefs::default();
        d.add(crate::features::SHIPPED_CHEATS, "cheats.toml");
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        assert_eq!(d.cheats.len(), 7);
        for c in &d.cheats {
            assert!(c.unusable().unwrap().contains("unknown until the executable"), "{}", c.id);
        }
    }

    #[test]
    fn resolve_and_apply() {
        let m = Fake(RefCell::new(vec![0; 64]));
        let mut d = CheatDefs::default();
        d.add(
            "[[location]]\nid = \"money\"\ntype = \"i32\"\nmodule = \"game.exe\"\nexport = \"g\"\noffset = 4\n\
             [[cheat]]\nid = \"money\"\nkind = \"freeze\"\nvalue = 999\nlocation = \"money\"\n\
             [[cheat]]\nid = \"fuel\"\nkind = \"multiply\"\ntype = \"f32\"\nfactor = 0\ndirection = \"decrease\"\nlocation = { module = \"game.exe\", pattern = \"AA BB\", rip = { disp = 3, len = 7 } }\n\
             [[cheat]]\nid = \"nop\"\nkind = \"patch\"\nbytes = \"90 90\"\nlocation = { module = \"game.exe\", offset = 0x30 }\n\
             [[cheat]]\nid = \"dmg\"\nkind = \"hook\"\nlocation = { address = 0x1000 }\n",
            "t",
        );
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        let money = &d.cheats[0];
        assert_eq!(money.location.as_ref().unwrap().resolve(&m), Ok(0x1014));
        let mut st = CheatState { on: true, value: 999.0, ..Default::default() };
        apply(money, &mut st, &m, None);
        let mut b = [0u8; 4];
        m.read(0x1014, &mut b);
        assert_eq!(i32::from_le_bytes(b), 999);
        // multiply by 0 on decreases: the value never goes down
        let fuel = &d.cheats[1];
        assert_eq!(fuel.location.as_ref().unwrap().resolve(&m), Ok(0x1020 + 7 + 4));
        let fa = 0x102B;
        m.write(fa, &50f32.to_le_bytes());
        let mut fs = CheatState { on: true, value: 0.0, ..Default::default() };
        apply(fuel, &mut fs, &m, None);
        m.write(fa, &45f32.to_le_bytes());
        apply(fuel, &mut fs, &m, None);
        m.read(fa, &mut b);
        assert_eq!(f32::from_le_bytes(b), 50.0);
        m.write(fa, &60f32.to_le_bytes());
        apply(fuel, &mut fs, &m, None);
        m.read(fa, &mut b);
        assert_eq!(f32::from_le_bytes(b), 60.0, "increases pass");
        // patch on / off restores
        let mut ps = CheatState::default();
        m.write(0x1030, &[1, 2]);
        apply(&d.cheats[2], &mut ps, &m, Some(true));
        let mut two = [0u8; 2];
        m.read(0x1030, &mut two);
        assert_eq!(two, [0x90, 0x90]);
        apply(&d.cheats[2], &mut ps, &m, Some(false));
        m.read(0x1030, &mut two);
        assert_eq!(two, [1, 2]);
        assert!(d.cheats[3].unusable().unwrap().contains("detours"));
    }
}
