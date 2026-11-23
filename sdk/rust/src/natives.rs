//! Natives: typed calls of the game's script commands from script functions.
//!
//! ```ignore
//! use scripthook_sdk::natives::{invoke, Vec3};
//! let ped: i32 = invoke("PLAYER::PLAYER_PED_ID", ())?;
//! let pos: Vec3 = invoke("ENTITY::GET_ENTITY_COORDS", (ped, true))?;
//! invoke::<()>("ENTITY::SET_ENTITY_COORDS_NO_OFFSET", (ped, pos, false, false, false))?;
//! ```
//!
//! Arguments: `i32`, `u32` (int), `bool`, `f32` / `f64` (float), [`Hash`], `&str` / `String`,
//! [`Vec3`] (three float slots), `u64`, out parameters `&mut i32`, `&mut f32`, `&mut Vec3`,
//! [`OutBool`]. Results: `()`, `i32`, `bool`, `f32`, [`Hash`], [`Vec3`], `u64`, `String`.
//! The host converts values to the types its natives database declares.

use crate::ffi::*;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::Mutex;

pub type Vec3 = ShkVec3;

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }
}

/// A 32-bit name hash value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Hash(pub u32);

impl Hash {
    pub fn of(name: &str) -> Hash {
        Hash(crate::hash::joaat(name))
    }
}

/// An out parameter of type `BOOL*` (stored as an `i32`).
pub struct OutBool<'a>(pub &'a mut i32);

/// Type of a native argument or result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Void,
    Int,
    Float,
    Bool,
    Hash,
    Str,
    Vec3,
    U64,
    Ptr,
    OutInt,
    OutFloat,
    OutBool,
    OutVec3,
}

impl Type {
    pub fn from_code(c: u32) -> Option<Type> {
        Some(match c {
            SHK_TYPE_VOID => Type::Void,
            SHK_TYPE_INT => Type::Int,
            SHK_TYPE_FLOAT => Type::Float,
            SHK_TYPE_BOOL => Type::Bool,
            SHK_TYPE_HASH => Type::Hash,
            SHK_TYPE_STRING => Type::Str,
            SHK_TYPE_VEC3 => Type::Vec3,
            SHK_TYPE_U64 => Type::U64,
            SHK_TYPE_PTR => Type::Ptr,
            SHK_TYPE_OUT_INT => Type::OutInt,
            SHK_TYPE_OUT_FLOAT => Type::OutFloat,
            SHK_TYPE_OUT_BOOL => Type::OutBool,
            SHK_TYPE_OUT_VEC3 => Type::OutVec3,
            _ => return None,
        })
    }
    pub fn code(self) -> u32 {
        match self {
            Type::Void => SHK_TYPE_VOID,
            Type::Int => SHK_TYPE_INT,
            Type::Float => SHK_TYPE_FLOAT,
            Type::Bool => SHK_TYPE_BOOL,
            Type::Hash => SHK_TYPE_HASH,
            Type::Str => SHK_TYPE_STRING,
            Type::Vec3 => SHK_TYPE_VEC3,
            Type::U64 => SHK_TYPE_U64,
            Type::Ptr => SHK_TYPE_PTR,
            Type::OutInt => SHK_TYPE_OUT_INT,
            Type::OutFloat => SHK_TYPE_OUT_FLOAT,
            Type::OutBool => SHK_TYPE_OUT_BOOL,
            Type::OutVec3 => SHK_TYPE_OUT_VEC3,
        }
    }
    pub fn is_out(self) -> bool {
        matches!(self, Type::OutInt | Type::OutFloat | Type::OutBool | Type::OutVec3)
    }
    /// Parses a type name of the natives database (`int`, `float`, `BOOL`, `Hash`, `const char*`,
    /// `Vector3`, `Vector3*`, `int*`, `Ped`, `Any*`, ...). `None` for an unknown spelling.
    pub fn parse(name: &str) -> Option<Type> {
        let t = name.trim();
        let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        let l = compact.to_ascii_lowercase();
        if l == "constchar*" || l == "char*" || l == "string" || l == "str" {
            return Some(Type::Str);
        }
        if let Some(base) = l.strip_suffix('*') {
            return Some(match base.trim_start_matches("const") {
                "float" | "f32" => Type::OutFloat,
                "bool" => Type::OutBool,
                "vector3" | "vec3" | "scrvector" => Type::OutVec3,
                "void" => Type::Ptr,
                _ => Type::OutInt,
            });
        }
        Some(match l.as_str() {
            "void" => Type::Void,
            "float" | "f32" | "double" => Type::Float,
            "bool" => Type::Bool,
            "hash" | "uint" | "unsignedint" | "u32" => Type::Hash,
            "vector3" | "vec3" | "scrvector" => Type::Vec3,
            "u64" | "uint64" | "uint64_t" | "i64" | "int64" => Type::U64,
            "ptr" | "pointer" => Type::Ptr,
            "out_int" => Type::OutInt,
            "out_float" => Type::OutFloat,
            "out_bool" => Type::OutBool,
            "out_vec3" => Type::OutVec3,
            "" => return None,
            _ if l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => Type::Int,
            _ => return None,
        })
    }
    /// The canonical name used in `natives.tsv`.
    pub fn name(self) -> &'static str {
        match self {
            Type::Void => "void",
            Type::Int => "int",
            Type::Float => "float",
            Type::Bool => "bool",
            Type::Hash => "hash",
            Type::Str => "string",
            Type::Vec3 => "vec3",
            Type::U64 => "u64",
            Type::Ptr => "ptr",
            Type::OutInt => "int*",
            Type::OutFloat => "float*",
            Type::OutBool => "bool*",
            Type::OutVec3 => "vec3*",
        }
    }
}

/// What the host's natives database knows about a native.
#[derive(Debug, Clone, Default)]
pub struct NativeInfo {
    pub id: u64,
    pub available: bool,
    pub args: Option<Vec<Type>>,
    pub ret: Option<Type>,
    pub name: String,
    pub reason: String,
}

/// Why a native call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeError {
    /// the host has no natives (no script hook)
    NoScriptHook,
    /// unknown to the database / not callable yet (reason)
    Unavailable(String),
    /// the host refused the call (not on a script fiber, wrong arguments; see the host log)
    CallFailed(String),
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NativeError::NoScriptHook => write!(f, "the host has no script hook"),
            NativeError::Unavailable(r) => write!(f, "{r}"),
            NativeError::CallFailed(n) => write!(f, "{n}: the call failed (see the host log)"),
        }
    }
}

impl std::error::Error for NativeError {}

pub type Result<T> = std::result::Result<T, NativeError>;

/// A native looked up by name or identifier.
#[derive(Debug, Clone)]
pub struct Native {
    pub handle: ShkNative,
    pub info: NativeInfo,
}

impl Native {
    /// `"NS::NAME"`, `"NAME"` or `"0x<hash>"`.
    pub fn lookup(name: &str) -> Result<Native> {
        let h = crate::host();
        let api = h.natives_api().ok_or(NativeError::NoScriptHook)?;
        let n = CString::new(name).map_err(|_| NativeError::Unavailable(format!("bad native name {name:?}")))?;
        let mut raw = ShkNativeInfo::default();
        // SAFETY: valid pointers.
        let handle = unsafe { (api.lookup)(n.as_ptr(), &mut raw) };
        let info = NativeInfo {
            id: raw.id,
            available: raw.available != 0 && handle != 0,
            args: (raw.arg_count != u32::MAX && raw.arg_count <= 32).then(|| raw.arg_types[..raw.arg_count as usize].iter().map(|&c| Type::from_code(c).unwrap_or(Type::Int)).collect()),
            ret: Type::from_code(raw.ret_type),
            name: if raw.name[0] != 0 { fixed_str(&raw.name) } else { name.to_string() },
            reason: fixed_str(&raw.reason),
        };
        Ok(Native { handle, info })
    }

    /// By identifier (a hash the database may not list).
    pub fn by_hash(hash: u64) -> Result<Native> {
        Native::lookup(&format!("0x{hash:016X}"))
    }

    /// Calls with raw values; `ret` is the expected result type (Void for none).
    pub fn call_raw(&self, args: &[ShkValue], ret: u32) -> Result<ShkValue> {
        if self.handle == 0 {
            return Err(NativeError::Unavailable(if self.info.reason.is_empty() { format!("needs native {} (unknown)", self.info.name) } else { self.info.reason.clone() }));
        }
        let api = crate::host().natives_api().ok_or(NativeError::NoScriptHook)?;
        let mut r = ShkValue { ty: ret, ..Default::default() };
        let rp: *mut ShkValue = if ret == SHK_TYPE_VOID { std::ptr::null_mut() } else { &mut r };
        // SAFETY: args slice, result out parameter.
        let ok = unsafe { (api.call)(self.handle, args.as_ptr(), args.len() as u32, rp) };
        if ok == 0 {
            return Err(NativeError::CallFailed(self.info.name.clone()));
        }
        Ok(r)
    }

    /// Typed call.
    pub fn call<A: Args, R: FromValue>(&self, args: A) -> Result<R> {
        let mut keep = Vec::new();
        let v = args.values(&mut keep);
        let r = self.call_raw(&v, R::TYPE)?;
        Ok(R::from_value(&r))
    }
}

static CACHE: Mutex<Option<HashMap<String, Native>>> = Mutex::new(None);

/// Looks a native up once (cached by name).
pub fn native(name: &str) -> Result<Native> {
    if let Some(n) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(name)) {
        return Ok(n.clone());
    }
    let n = Native::lookup(name)?;
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(name.to_string(), n.clone());
    Ok(n)
}

/// Calls a native by name from a script function.
pub fn invoke<R: FromValue>(name: &str, args: impl Args) -> Result<R> {
    native(name)?.call(args)
}

/// Calls a native by identifier from a script function.
pub fn invoke_hash<R: FromValue>(hash: u64, args: impl Args) -> Result<R> {
    native(&format!("0x{hash:016X}"))?.call(args)
}

/// A native in a `static`, looked up at its first call (used by generated `natives.rs`).
pub struct NativeCell {
    name: &'static str,
    cell: std::sync::OnceLock<Native>,
}

impl NativeCell {
    pub const fn new(name: &'static str) -> NativeCell {
        NativeCell { name, cell: std::sync::OnceLock::new() }
    }
    pub fn get(&self) -> Result<&Native> {
        if let Some(n) = self.cell.get() {
            return Ok(n);
        }
        let n = Native::lookup(self.name)?;
        Ok(self.cell.get_or_init(|| n))
    }
    pub fn call<A: Args, R: FromValue>(&self, args: A) -> Result<R> {
        self.get()?.call(args)
    }
}

// ---- argument and result conversions -----------------------------------------------------------

fn val(ty: u32, lo: u64, hi: u64) -> ShkValue {
    ShkValue { ty, reserved: 0, u: [lo, hi] }
}

/// One argument.
pub trait IntoArg {
    /// `keep` holds C strings alive for the call.
    fn into_arg(self, keep: &mut Vec<CString>) -> ShkValue;
}

impl IntoArg for i32 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_INT, self as u32 as u64, 0)
    }
}
impl IntoArg for u32 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_INT, self as u64, 0)
    }
}
impl IntoArg for bool {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_BOOL, self as u64, 0)
    }
}
impl IntoArg for f32 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_FLOAT, self.to_bits() as u64, 0)
    }
}
impl IntoArg for f64 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_FLOAT, (self as f32).to_bits() as u64, 0)
    }
}
impl IntoArg for Hash {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_HASH, self.0 as u64, 0)
    }
}
impl IntoArg for u64 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_U64, self, 0)
    }
}
impl IntoArg for &str {
    fn into_arg(self, keep: &mut Vec<CString>) -> ShkValue {
        let c = CString::new(self.replace('\0', "")).unwrap_or_default();
        let p = c.as_ptr() as u64;
        keep.push(c);
        val(SHK_TYPE_STRING, p, 0)
    }
}
impl IntoArg for String {
    fn into_arg(self, keep: &mut Vec<CString>) -> ShkValue {
        self.as_str().into_arg(keep)
    }
}
impl IntoArg for &String {
    fn into_arg(self, keep: &mut Vec<CString>) -> ShkValue {
        self.as_str().into_arg(keep)
    }
}
impl IntoArg for Vec3 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        let lo = self.x.to_bits() as u64 | (self.y.to_bits() as u64) << 32;
        val(SHK_TYPE_VEC3, lo, self.z.to_bits() as u64)
    }
}
impl IntoArg for &mut i32 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_OUT_INT, self as *mut i32 as u64, 0)
    }
}
impl IntoArg for &mut f32 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_OUT_FLOAT, self as *mut f32 as u64, 0)
    }
}
impl IntoArg for &mut Vec3 {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_OUT_VEC3, self as *mut Vec3 as u64, 0)
    }
}
impl IntoArg for OutBool<'_> {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        val(SHK_TYPE_OUT_BOOL, self.0 as *mut i32 as u64, 0)
    }
}
impl IntoArg for ShkValue {
    fn into_arg(self, _: &mut Vec<CString>) -> ShkValue {
        self
    }
}

/// An argument list: `()`, `(a,)`, `(a, b)`, ... up to 16, or `Vec<ShkValue>`.
pub trait Args {
    fn values(self, keep: &mut Vec<CString>) -> Vec<ShkValue>;
}

impl Args for () {
    fn values(self, _: &mut Vec<CString>) -> Vec<ShkValue> {
        Vec::new()
    }
}

impl Args for Vec<ShkValue> {
    fn values(self, _: &mut Vec<CString>) -> Vec<ShkValue> {
        self
    }
}

macro_rules! tuple_args {
    ($($n:ident),+) => {
        impl<$($n: IntoArg),+> Args for ($($n,)+) {
            #[allow(non_snake_case)]
            fn values(self, keep: &mut Vec<CString>) -> Vec<ShkValue> {
                let ($($n,)+) = self;
                vec![$($n.into_arg(keep)),+]
            }
        }
    };
}
tuple_args!(A);
tuple_args!(A, B);
tuple_args!(A, B, C);
tuple_args!(A, B, C, D);
tuple_args!(A, B, C, D, E);
tuple_args!(A, B, C, D, E, F);
tuple_args!(A, B, C, D, E, F, G);
tuple_args!(A, B, C, D, E, F, G, H);
tuple_args!(A, B, C, D, E, F, G, H, I);
tuple_args!(A, B, C, D, E, F, G, H, I, J);
tuple_args!(A, B, C, D, E, F, G, H, I, J, K);
tuple_args!(A, B, C, D, E, F, G, H, I, J, K, L);
tuple_args!(A, B, C, D, E, F, G, H, I, J, K, L, M);
tuple_args!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
tuple_args!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
tuple_args!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);

/// A result type.
pub trait FromValue: Sized {
    const TYPE: u32;
    fn from_value(v: &ShkValue) -> Self;
}

impl FromValue for () {
    const TYPE: u32 = SHK_TYPE_VOID;
    fn from_value(_: &ShkValue) {}
}
impl FromValue for i32 {
    const TYPE: u32 = SHK_TYPE_INT;
    fn from_value(v: &ShkValue) -> i32 {
        v.u[0] as u32 as i32
    }
}
impl FromValue for bool {
    const TYPE: u32 = SHK_TYPE_BOOL;
    fn from_value(v: &ShkValue) -> bool {
        v.u[0] as u32 != 0
    }
}
impl FromValue for f32 {
    const TYPE: u32 = SHK_TYPE_FLOAT;
    fn from_value(v: &ShkValue) -> f32 {
        if v.ty == SHK_TYPE_INT {
            v.u[0] as u32 as i32 as f32
        } else {
            f32::from_bits(v.u[0] as u32)
        }
    }
}
impl FromValue for Hash {
    const TYPE: u32 = SHK_TYPE_HASH;
    fn from_value(v: &ShkValue) -> Hash {
        Hash(v.u[0] as u32)
    }
}
impl FromValue for u64 {
    const TYPE: u32 = SHK_TYPE_U64;
    fn from_value(v: &ShkValue) -> u64 {
        v.u[0]
    }
}
impl FromValue for Vec3 {
    const TYPE: u32 = SHK_TYPE_VEC3;
    fn from_value(v: &ShkValue) -> Vec3 {
        Vec3 { x: f32::from_bits(v.u[0] as u32), y: f32::from_bits((v.u[0] >> 32) as u32), z: f32::from_bits(v.u[1] as u32) }
    }
}
impl FromValue for String {
    const TYPE: u32 = SHK_TYPE_STRING;
    fn from_value(v: &ShkValue) -> String {
        if v.u[0] == 0 {
            String::new()
        } else {
            // SAFETY: the host returns a NUL-terminated string valid until the next call.
            unsafe { CStr::from_ptr(v.u[0] as *const std::ffi::c_char).to_string_lossy().into_owned() }
        }
    }
}
impl FromValue for ShkValue {
    const TYPE: u32 = SHK_TYPE_INT;
    fn from_value(v: &ShkValue) -> ShkValue {
        *v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_names() {
        for (s, t) in [("int", Type::Int), ("Ped", Type::Int), ("BOOL", Type::Bool), ("Hash", Type::Hash), ("const char*", Type::Str), ("Vector3", Type::Vec3), ("Vector3*", Type::OutVec3), ("int*", Type::OutInt), ("Any*", Type::OutInt), ("float*", Type::OutFloat), ("BOOL*", Type::OutBool), ("void", Type::Void), ("float", Type::Float), ("u64", Type::U64)] {
            assert_eq!(Type::parse(s), Some(t), "{s}");
        }
        assert_eq!(Type::parse("int, float"), None);
        for t in [Type::Int, Type::Float, Type::Vec3, Type::OutVec3, Type::Str, Type::Hash] {
            assert_eq!(Type::parse(t.name()), Some(t));
            assert_eq!(Type::from_code(t.code()), Some(t));
        }
    }

    #[test]
    fn conversions() {
        let mut keep = Vec::new();
        let v = (7i32, true, 1.5f32, Hash(9), "abc", Vec3::new(1.0, 2.0, 3.0)).values(&mut keep);
        assert_eq!(v.len(), 6);
        assert_eq!((v[0].ty, v[0].u[0]), (SHK_TYPE_INT, 7));
        assert_eq!((v[1].ty, v[1].u[0]), (SHK_TYPE_BOOL, 1));
        assert_eq!(f32::from_bits(v[2].u[0] as u32), 1.5);
        assert_eq!((v[3].ty, v[3].u[0]), (SHK_TYPE_HASH, 9));
        assert_eq!(unsafe { CStr::from_ptr(v[4].u[0] as *const std::ffi::c_char) }.to_str().unwrap(), "abc");
        assert_eq!(Vec3::from_value(&v[5]), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(i32::from_value(&(-5i32).into_arg(&mut keep)), -5);
        let mut x = 0i32;
        let o = (&mut x).into_arg(&mut keep);
        assert_eq!(o.ty, SHK_TYPE_OUT_INT);
        assert_eq!(std::mem::size_of::<ShkValue>(), 24);
    }
}
