//! Natives: the game's script commands, called the way its script interpreter calls them —
//! through its own handler table with a call context. Nothing about an executable is assumed:
//! the table's location and shape, the context layout and every native's identifier come from
//! `natives.tsv` (docs/natives-database.md).

use crate::{locate, mem, scripts};
use scripthook_sdk::config::Value;
use scripthook_sdk::ffi::*;
use scripthook_sdk::natives::Type;
use shk_natives::db::{self, Args, Id, Ret};
use std::collections::HashMap;
use std::ffi::{CStr, CString};

/// Byte offsets of the call context.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub size: usize,
    pub return_ptr: usize,
    pub arg_count: usize,
    pub args_ptr: usize,
    pub data_count: usize,
    pub fixup_out: usize,
    pub fixup_in: usize,
    pub fixups: usize,
    pub slot: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { size: 0xE0, return_ptr: 0x00, arg_count: 0x08, args_ptr: 0x10, data_count: 0x18, fixup_out: 0x20, fixup_in: 0x40, fixups: 4, slot: 8 }
    }
}

#[derive(Debug, Clone)]
pub enum TableKind {
    Buckets { buckets: usize, modulo: bool, next: usize, handlers: usize, capacity: usize, count: usize, count_bytes: usize, hashes: usize, xor: u64 },
    Pairs { stride: usize, hash: usize, handler: usize, count: Option<usize>, hash_bytes: usize },
    Array { stride: usize, handler: usize, count: Option<usize> },
}

#[derive(Debug, Clone)]
pub struct Table {
    pub kind: TableKind,
    pub location: Option<Value>,
}

pub struct Natives {
    pub db: db::Db,
    pub layout: Layout,
    pub table: Option<Table>,
    pub tick: Option<Value>,
    base: Option<Result<usize, String>>,
    handlers: HashMap<String, usize>,
}

fn sz(v: &Value, k: &str, d: usize) -> usize {
    v.i64_of(k).map(|x| x as usize).unwrap_or(d)
}

fn u64_of(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Int(i) => Some(*i as u64),
        Value::Str(s) => s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).and_then(|h| u64::from_str_radix(h, 16).ok()).or_else(|| s.parse().ok()),
        _ => None,
    }
}

fn type_of(t: Type) -> u32 {
    t.code()
}

impl Natives {
    /// From one or more `natives.tsv` texts (later rows replace earlier ones).
    pub fn from_tsv(texts: &[(&str, String)]) -> (Natives, Vec<String>) {
        let mut all = db::Db::default();
        let mut errors = Vec::new();
        for (src, t) in texts {
            let mut d = db::parse(t);
            d.check();
            errors.extend(d.errors.iter().map(|e| format!("{src}: {e}")));
            for n in d.natives {
                all.natives.retain(|x| x.name != n.name);
                all.natives.push(n);
            }
            for dv in d.directives {
                all.directives.retain(|x| x.name != dv.name);
                all.directives.push(dv);
            }
        }
        let mut n = Natives { db: all, layout: Layout::default(), table: None, tick: None, base: None, handlers: HashMap::new() };
        for d in n.db.directives.clone() {
            let v = &d.value;
            match d.name.as_str() {
                "native_context" => {
                    let l = Layout::default();
                    n.layout = Layout {
                        size: sz(v, "size", l.size),
                        return_ptr: sz(v, "return_ptr", l.return_ptr),
                        arg_count: sz(v, "arg_count", l.arg_count),
                        args_ptr: sz(v, "args_ptr", l.args_ptr),
                        data_count: sz(v, "data_count", l.data_count),
                        fixup_out: sz(v, "fixup_out", l.fixup_out),
                        fixup_in: sz(v, "fixup_in", l.fixup_in),
                        fixups: sz(v, "fixups", l.fixups),
                        slot: sz(v, "slot", l.slot),
                    };
                }
                "native_table" => {
                    let kind = match v.str_of("kind").as_deref().unwrap_or("buckets") {
                        "pairs" => TableKind::Pairs { stride: sz(v, "stride", 16), hash: sz(v, "hash", 0), handler: sz(v, "handler", 8), count: v.i64_of("count").map(|c| c as usize), hash_bytes: if v.str_of("hash_type").as_deref() == Some("u32") { 4 } else { 8 } },
                        "array" | "index" => TableKind::Array { stride: sz(v, "stride", 8), handler: sz(v, "handler", 0), count: v.i64_of("count").map(|c| c as usize) },
                        _ => TableKind::Buckets {
                            buckets: sz(v, "buckets", 256),
                            modulo: v.str_of("bucket_index").as_deref() == Some("modulo"),
                            next: sz(v, "next", 0),
                            handlers: sz(v, "handlers", 8),
                            capacity: sz(v, "capacity", 7),
                            count: sz(v, "count", 0x40),
                            count_bytes: if v.str_of("count_type").as_deref() == Some("u8") { 1 } else { 4 },
                            hashes: sz(v, "hashes", 0x48),
                            xor: u64_of(v.get("hash_xor")).unwrap_or(0),
                        },
                    };
                    n.table = Some(Table { kind, location: Some(v.clone()) });
                }
                "script_tick" => n.tick = Some(v.clone()),
                _ => {}
            }
        }
        (n, errors)
    }

    pub fn known(&self) -> usize {
        self.db.natives.iter().filter(|n| n.id.is_some()).count()
    }

    /// The table's base address (resolved once).
    pub fn base(&mut self) -> Result<usize, String> {
        if let Some(b) = &self.base {
            return b.clone();
        }
        let r = match &self.table {
            None => Err("the natives table's location is not given (@native_table in natives.tsv)".to_string()),
            Some(t) => match &t.location {
                Some(l) => locate::resolve(l).map_err(|e| format!("natives table: {e}")),
                None => Err("no location".into()),
            },
        };
        // a table that is not there yet (filled at start-up) is looked up again later
        if r.is_ok() {
            self.base = Some(r.clone());
        }
        r
    }

    /// The handler of an identifier.
    pub fn handler(&mut self, id: Id) -> Result<usize, String> {
        let key = id.text();
        if let Some(h) = self.handlers.get(&key) {
            return Ok(*h);
        }
        let base = self.base()?;
        let t = self.table.clone().unwrap();
        let h = match (&t.kind, id) {
            (TableKind::Buckets { buckets, modulo, next, handlers, capacity, count, count_bytes, hashes, xor }, Id::Hash(hash)) => {
                let b = if *modulo { (hash % *buckets as u64) as usize } else { (hash & 0xFF) as usize % *buckets };
                let mut node = mem::read_u64(base + b * 8).ok_or("unreadable bucket")? as usize;
                let mut found = None;
                let mut guard = 0;
                while node != 0 && found.is_none() && guard < 100_000 {
                    let mut cb = [0u8; 4];
                    mem::read(node + count, &mut cb[..*count_bytes]).then_some(()).ok_or("unreadable node")?;
                    let n = (u32::from_le_bytes(cb) as usize).min(*capacity);
                    for k in 0..n {
                        let hk = mem::read_u64(node + hashes + k * 8).ok_or("unreadable hash")? ^ xor;
                        if hk == hash {
                            found = Some(mem::read_u64(node + handlers + k * 8).ok_or("unreadable handler")? as usize);
                            break;
                        }
                    }
                    node = mem::read_u64(node + next).unwrap_or(0) as usize;
                    guard += 1;
                }
                found.ok_or_else(|| format!("{key}: not in the natives table"))?
            }
            (TableKind::Pairs { stride, hash, handler, count, hash_bytes }, Id::Hash(want)) => {
                let mut found = None;
                for i in 0..count.unwrap_or(1 << 20) {
                    let e = base + i * stride;
                    let hv = if *hash_bytes == 4 { mem::read_u32(e + hash).map(|x| x as u64) } else { mem::read_u64(e + hash) };
                    match hv {
                        Some(0) if count.is_none() => break,
                        Some(x) if x == want => {
                            found = Some(mem::read_u64(e + handler).ok_or("unreadable handler")? as usize);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                found.ok_or_else(|| format!("{key}: not in the natives table"))?
            }
            (TableKind::Array { stride, handler, count }, Id::Index(i)) => {
                if count.map(|c| i as usize >= c).unwrap_or(false) {
                    return Err(format!("index {i} is outside the table"));
                }
                mem::read_u64(base + i as usize * stride + handler).ok_or("unreadable handler")? as usize
            }
            (TableKind::Array { .. }, Id::Hash(_)) => return Err("the natives table is indexed: give an index".into()),
            (_, Id::Index(_)) => return Err("the natives table is keyed by hash: give a hash".into()),
        };
        let m = mem::query(h).ok_or("unmapped handler")?;
        if m.State != shk_sys::MEM_COMMIT || m.Protect & 0xF0 == 0 {
            return Err(format!("{key}: handler {h:#x} is not executable code"));
        }
        self.handlers.insert(key, h);
        Ok(h)
    }

    /// Why `name` cannot be called (None = callable).
    pub fn unavailable(&mut self, name: &str) -> Option<String> {
        let n = match self.db.find(name) {
            Some(n) => n.clone(),
            None if name.starts_with("0x") => return self.handler(Id::parse(name).ok()?).err(),
            None => return Some(format!("needs native {name} (not in the natives database)")),
        };
        let Some(id) = n.id else { return Some(format!("needs native {} (identifier unknown until the executable)", n.name)) };
        self.handler(id).err()
    }

    /// Fills `info` as scripthook.h describes.
    pub fn info(&mut self, name: &str, info: &mut ShkNativeInfo) {
        *info = ShkNativeInfo::default();
        let why = self.unavailable(name);
        let put = |dst: &mut [u8], t: &str| {
            let b = t.as_bytes();
            let k = b.len().min(dst.len() - 1);
            dst[..k].copy_from_slice(&b[..k]);
            dst[k] = 0;
        };
        match self.db.find(name) {
            Some(n) => {
                info.id = match n.id {
                    Some(Id::Hash(h)) => h,
                    Some(Id::Index(i)) => i as u64,
                    None => 0,
                };
                match &n.args {
                    Args::Types(v) => {
                        info.arg_count = v.len() as u32;
                        for (k, (_, t)) in v.iter().take(32).enumerate() {
                            info.arg_types[k] = type_of(*t);
                        }
                    }
                    Args::Slots(s) => info.arg_count = *s as u32,
                    Args::Unknown => info.arg_count = u32::MAX,
                }
                info.ret_type = match &n.ret {
                    Ret::Type(_, t) => type_of(*t),
                    Ret::Unknown => u32::MAX,
                };
                put(&mut info.name, &n.name);
            }
            None => {
                info.arg_count = u32::MAX;
                info.ret_type = u32::MAX;
                put(&mut info.name, name);
            }
        }
        info.available = why.is_none() as u32;
        put(&mut info.reason, why.as_deref().unwrap_or(""));
    }

    /// Calls a native from a script function.
    pub fn call(&mut self, name: &str, args: &[ShkValue], ret_want: u32) -> Result<ShkValue, String> {
        if !scripts::in_script() {
            return Err(format!("{name}: natives can only be called from a script function"));
        }
        let decl = self.db.find(name).cloned();
        let id = match &decl {
            Some(n) => n.id.ok_or_else(|| format!("needs native {} (identifier unknown until the executable)", n.name))?,
            None => Id::parse(name).map_err(|_| format!("needs native {name} (not in the natives database)"))?,
        };
        let handler = self.handler(id)?;
        let types: Option<Vec<Type>> = match decl.as_ref().map(|n| &n.args) {
            Some(Args::Types(v)) => Some(v.iter().map(|x| x.1).collect()),
            _ => None,
        };
        let ret = match decl.as_ref().map(|n| &n.ret) {
            Some(Ret::Type(_, t)) => *t,
            _ => Type::from_code(ret_want).unwrap_or(Type::Int),
        };
        call_raw(&self.layout, handler, args, types.as_deref(), ret, name)
    }
}

thread_local! {
    static RET_STR: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::default());
}

/// Builds the context, calls the handler, applies vector fix-ups, copies out parameters back.
pub fn call_raw(l: &Layout, handler: usize, args: &[ShkValue], types: Option<&[Type]>, ret: Type, name: &str) -> Result<ShkValue, String> {
    let slot = l.slot.max(4);
    let mut keep: Vec<CString> = Vec::new();
    let mut slots: Vec<u64> = Vec::with_capacity(32);
    // out storage: (plugin pointer, type, storage)
    let mut outs: Vec<(u64, Type, Box<[u64; 4]>)> = Vec::new();
    let mut cursor = 0usize;
    for (k, a) in args.iter().enumerate() {
        let decl = types.and_then(|t| t.get(cursor)).copied();
        let given = Type::from_code(a.ty).ok_or_else(|| format!("{name}: argument {k}: unknown value type {}", a.ty))?;
        let lo = a.u[0];
        let f32_of = |v: u64| f32::from_bits(v as u32);
        match given {
            Type::Vec3 => {
                let (x, y, z) = (lo as u32 as u64, lo >> 32, a.u[1] as u32 as u64);
                slots.extend_from_slice(&[x, y, z]);
                cursor += if decl == Some(Type::Vec3) { 1 } else { 3 };
                continue;
            }
            t if t.is_out() => {
                if lo == 0 {
                    return Err(format!("{name}: argument {k}: an out parameter without storage"));
                }
                let mut b = Box::new([0u64; 4]);
                // start with the plugin's current value
                let mut cur = [0u8; 12];
                let n = if t == Type::OutVec3 { 12 } else { 4 };
                mem::read(lo as usize, &mut cur[..n]);
                if t == Type::OutVec3 {
                    for i in 0..3 {
                        b[i] = u32::from_le_bytes(cur[i * 4..i * 4 + 4].try_into().unwrap()) as u64;
                    }
                } else {
                    b[0] = u32::from_le_bytes(cur[..4].try_into().unwrap()) as u64;
                }
                slots.push(b.as_ptr() as u64);
                outs.push((lo, t, b));
            }
            Type::Str => {
                let v = if lo == 0 {
                    0
                } else {
                    // SAFETY: the plugin passes a NUL-terminated string.
                    let s = unsafe { CStr::from_ptr(lo as *const std::ffi::c_char) }.to_owned();
                    if decl == Some(Type::Hash) {
                        scripthook_sdk::hash::joaat_bytes(s.as_bytes()) as u64
                    } else {
                        let p = s.as_ptr() as u64;
                        keep.push(s);
                        p
                    }
                };
                slots.push(v);
            }
            Type::Float => slots.push(match decl {
                Some(Type::Int | Type::Bool | Type::Hash) => f32_of(lo) as i32 as u32 as u64,
                _ => lo as u32 as u64,
            }),
            Type::Int | Type::Bool | Type::Hash => slots.push(match decl {
                Some(Type::Float) => (lo as u32 as i32 as f32).to_bits() as u64,
                _ => lo as u32 as u64,
            }),
            Type::U64 | Type::Ptr => slots.push(lo),
            Type::Void => return Err(format!("{name}: argument {k}: void")),
            _ => slots.push(lo),
        }
        cursor += 1;
    }
    if let Some(t) = types {
        if cursor != t.len() {
            return Err(format!("{name}: {} argument(s) given, {} declared", cursor, t.len()));
        }
    }
    // slots of `slot` bytes (8 normally)
    let mut argbuf = vec![0u8; (slots.len() + 4) * slot];
    for (i, v) in slots.iter().enumerate() {
        argbuf[i * slot..i * slot + slot.min(8)].copy_from_slice(&v.to_le_bytes()[..slot.min(8)]);
    }
    let mut retbuf = vec![0u8; 4 * slot.max(8)];
    let mut ctx = vec![0u8; l.size.max(l.fixup_in + l.fixups * 16)];
    let put = |c: &mut Vec<u8>, off: usize, v: u64, n: usize| c[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    put(&mut ctx, l.return_ptr, retbuf.as_mut_ptr() as u64, 8);
    put(&mut ctx, l.args_ptr, argbuf.as_mut_ptr() as u64, 8);
    put(&mut ctx, l.arg_count, slots.len() as u64, 4);
    put(&mut ctx, l.data_count, 0, 4);
    // SAFETY: the handler is executable code of the game's table (checked); the context and
    // buffers are ours and outlive the call.
    unsafe {
        let f: extern "C" fn(*mut u8) = std::mem::transmute(handler);
        f(ctx.as_mut_ptr());
    }
    // vector fix-ups: copy the queued 16-byte vectors to the caller's vectors
    let n = (u32::from_le_bytes(ctx[l.data_count..l.data_count + 4].try_into().unwrap()) as usize).min(l.fixups);
    for i in 0..n {
        let dst = u64::from_le_bytes(ctx[l.fixup_out + i * 8..l.fixup_out + i * 8 + 8].try_into().unwrap()) as usize;
        let src = l.fixup_in + i * 16;
        if dst != 0 {
            for c in 0..3 {
                let v = &ctx[src + c * 4..src + c * 4 + 4];
                mem::write(dst + c * slot, v);
            }
        }
    }
    // out parameters back to the plugin
    for (p, t, b) in &outs {
        match t {
            Type::OutVec3 => {
                let v: Vec<u8> = (0..3).flat_map(|i| (b[i] as u32).to_le_bytes()).collect();
                mem::write(*p as usize, &v);
            }
            _ => {
                mem::write(*p as usize, &(b[0] as u32).to_le_bytes());
            }
        }
    }
    let r = |i: usize| u64::from_le_bytes(retbuf[i * slot..i * slot + 8].try_into().unwrap());
    let mut out = ShkValue { ty: type_of(ret), reserved: 0, u: [0, 0] };
    match ret {
        Type::Void => {}
        Type::Vec3 => {
            out.u[0] = (r(0) as u32 as u64) | ((r(1) as u32 as u64) << 32);
            out.u[1] = r(2) as u32 as u64;
        }
        Type::Str => {
            let p = r(0) as usize;
            let s = if p == 0 {
                CString::default()
            } else {
                let mut b = vec![0u8; 512];
                let mut k = 512;
                while k > 0 && !mem::read(p, &mut b[..k]) {
                    k /= 2;
                }
                let end = b[..k].iter().position(|&c| c == 0).unwrap_or(k);
                CString::new(b[..end].to_vec()).unwrap_or_default()
            };
            out.u[0] = RET_STR.with(|c| {
                *c.borrow_mut() = s;
                c.borrow().as_ptr() as u64
            });
        }
        Type::U64 | Type::Ptr => out.u[0] = r(0),
        Type::Bool => out.u[0] = (r(0) as u32 != 0) as u64,
        _ => out.u[0] = r(0) as u32 as u64,
    }
    drop(keep);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // A synthetic handler table in this process: 4 chained buckets, XOR-obfuscated hashes, a
    // context layout that is not the default.
    #[repr(C)]
    struct NCtx {
        args: *mut u64,
        argc: u32,
        data_count: u32,
        ret: *mut u64,
        fix_out: [*mut f32; 4],
        fix_in: [[f32; 4]; 4],
    }
    #[repr(C)]
    struct Bucket {
        next: *mut Bucket,
        h: [usize; 7],
        count: u32,
        pad: u32,
        hashes: [u64; 7],
    }

    extern "C" fn n_add(c: *mut NCtx) {
        unsafe {
            let a = *(*c).args as i32;
            let b = *(*c).args.add(1) as i32;
            *(*c).ret = (a + b) as u32 as u64;
        }
    }
    extern "C" fn n_scale(c: *mut NCtx) {
        unsafe {
            let f = |k| f32::from_bits(*(*c).args.add(k) as u32);
            let s = f(3);
            for k in 0..3 {
                *(*c).ret.add(k) = (f(k) * s).to_bits() as u64;
            }
        }
    }
    extern "C" fn n_out(c: *mut NCtx) {
        unsafe {
            *(*(*c).args as *mut u32) = 77;
            (*c).fix_out[0] = *(*c).args.add(1) as *mut f32;
            (*c).fix_in[0] = [1.0, 2.0, 3.0, 0.0];
            (*c).data_count = 1;
        }
    }
    static LEN: AtomicU32 = AtomicU32::new(0);
    extern "C" fn n_len(c: *mut NCtx) {
        unsafe {
            let s = CStr::from_ptr(*(*c).args as *const std::ffi::c_char);
            LEN.store(s.to_bytes().len() as u32, Ordering::SeqCst);
            *(*c).ret = s.to_bytes().len() as u64;
        }
    }

    fn id(name: &str) -> u64 {
        let j = scripthook_sdk::joaat(name) as u64;
        j << 32 | (j ^ 0x1234)
    }

    #[test]
    fn table_lookup_and_typed_calls() {
        const XOR: u64 = 0x5A5A_5A5A_0000_0000;
        let entries: Vec<(&str, usize)> = vec![("T::ADD", n_add as *const () as usize), ("T::SCALE", n_scale as *const () as usize), ("T::OUT", n_out as *const () as usize), ("T::LEN", n_len as *const () as usize)];
        // registry: magic then 4 bucket pointers
        let mut reg: Box<[u64; 6]> = Box::new([0; 6]);
        reg[0] = u64::from_le_bytes(*b"SHK-NATI");
        reg[1] = u64::from_le_bytes(*b"VES-REG!");
        let mut nodes: Vec<Box<Bucket>> = Vec::new();
        for (n, h) in &entries {
            let hv = id(n);
            let b = (hv % 4) as usize;
            // one node per entry, chained in front
            let node = Box::new(Bucket { next: reg[2 + b] as *mut Bucket, h: [*h, 0, 0, 0, 0, 0, 0], count: 1, pad: 0, hashes: [hv ^ XOR, 0, 0, 0, 0, 0, 0] });
            reg[2 + b] = &*node as *const Bucket as u64;
            nodes.push(node);
        }
        let tsv = format!(
            "identifier\targs\treturns\tname\n@native_context\t{{ size = 0x80, args_ptr = 0x00, arg_count = 0x08, data_count = 0x0C, return_ptr = 0x10, fixup_out = 0x18, fixup_in = 0x38 }}\n@native_table\t{{ kind = \"buckets\", buckets = 4, bucket_index = \"modulo\", hash_xor = \"0x5A5A5A5A00000000\", address = {} }}\n{:#018x}\tint, int\tint\tT::ADD\n{:#018x}\tVector3, float\tVector3\tT::SCALE\n{:#018x}\tint*, Vector3*\tvoid\tT::OUT\n{:#018x}\tconst char*\tint\tT::LEN\n-\t0\tvoid\tT::UNKNOWN\n",
            &reg[2] as *const u64 as usize,
            id("T::ADD"),
            id("T::SCALE"),
            id("T::OUT"),
            id("T::LEN")
        );
        let (mut n, errs) = Natives::from_tsv(&[("t", tsv)]);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(n.unavailable("T::ADD").is_none());
        assert!(n.unavailable("T::UNKNOWN").unwrap().contains("identifier unknown"));
        let mut info = ShkNativeInfo::default();
        n.info("SCALE", &mut info);
        assert_eq!((info.available, info.arg_count, info.arg_types[0], info.ret_type), (1, 2, SHK_TYPE_VEC3, SHK_TYPE_VEC3));
        // calls need a script fiber: run them in one
        static OK: AtomicU32 = AtomicU32::new(0);
        struct P(*mut Natives);
        unsafe impl Send for P {}
        let p = P(&mut n);
        unsafe extern "C" fn body(u: *mut std::ffi::c_void) {
            let n = &mut *(u as *mut Natives);
            let v = |t: u32, lo: u64, hi: u64| ShkValue { ty: t, reserved: 0, u: [lo, hi] };
            let r = n.call("T::ADD", &[v(SHK_TYPE_INT, 40, 0), v(SHK_TYPE_INT, 2, 0)], SHK_TYPE_INT).unwrap();
            assert_eq!(r.u[0], 42);
            // an int for a float argument is converted
            let vec = (1.0f32.to_bits() as u64) | (2.0f32.to_bits() as u64) << 32;
            let r = n.call("T::SCALE", &[v(SHK_TYPE_VEC3, vec, 3.0f32.to_bits() as u64), v(SHK_TYPE_INT, 2, 0)], 0).unwrap();
            assert_eq!(r.ty, SHK_TYPE_VEC3);
            assert_eq!((f32::from_bits(r.u[0] as u32), f32::from_bits((r.u[0] >> 32) as u32), f32::from_bits(r.u[1] as u32)), (2.0, 4.0, 6.0));
            let mut i = 0i32;
            let mut out = ShkVec3::default();
            n.call("T::OUT", &[v(SHK_TYPE_OUT_INT, &mut i as *mut i32 as u64, 0), v(SHK_TYPE_OUT_VEC3, &mut out as *mut ShkVec3 as u64, 0)], 0).unwrap();
            assert_eq!(i, 77);
            assert_eq!(out, ShkVec3 { x: 1.0, y: 2.0, z: 3.0 });
            let s = c"hello";
            let r = n.call("T::LEN", &[v(SHK_TYPE_STRING, s.as_ptr() as u64, 0)], 0).unwrap();
            assert_eq!(r.u[0], 5);
            assert!(n.call("T::ADD", &[v(SHK_TYPE_INT, 1, 0)], 0).unwrap_err().contains("1 argument(s) given, 2 declared"));
            assert!(n.call("T::UNKNOWN", &[], 0).is_err());
            OK.store(1, Ordering::SeqCst);
        }
        let _g = crate::TEST_SCRIPTS.lock().unwrap_or_else(|e| e.into_inner());
        assert!(n.call("T::ADD", &[], 0).unwrap_err().contains("script function"));
        scripts::set_mode(scripts::Mode::GameThread, "");
        scripts::register(99, "t", body, p.0 as usize);
        std::thread::spawn(move || {
            let _p = p;
            scripts::tick(scripts::Mode::GameThread);
        })
        .join()
        .unwrap();
        scripts::set_mode(scripts::Mode::Off, "");
        assert_eq!(OK.load(Ordering::SeqCst), 1);
        drop(nodes);
    }
}
