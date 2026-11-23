//! Memory of the process the hook runs in: bounds-checked reads and writes, modules (the
//! loader's list and PE images mapped by other means, such as a runtime's own loader), sections,
//! exports, byte patterns, RIP-relative targets, pointer chains.

use shk_sys::*;
use std::ffi::c_void;

pub fn query(addr: usize) -> Option<MEMORY_BASIC_INFORMATION> {
    let mut m = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: out parameter.
    let n = unsafe { VirtualQuery(addr as *const c_void, &mut m, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
    (n != 0).then_some(m)
}

pub fn readable(p: u32) -> bool {
    p & (PAGE_GUARD | PAGE_NOACCESS) == 0 && p != 0
}

pub fn writable(p: u32) -> bool {
    matches!(p & 0xFF, PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY) && p & PAGE_GUARD == 0
}

/// Reads without ever faulting.
pub fn read(addr: usize, out: &mut [u8]) -> bool {
    if addr == 0 || out.is_empty() {
        return out.is_empty();
    }
    let mut n = 0usize;
    // SAFETY: the kernel checks the source range.
    let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, out.as_mut_ptr() as *mut c_void, out.len(), &mut n) };
    ok != 0 && n == out.len()
}

pub fn read_u64(addr: usize) -> Option<u64> {
    let mut b = [0u8; 8];
    read(addr, &mut b).then(|| u64::from_le_bytes(b))
}

pub fn read_u32(addr: usize) -> Option<u32> {
    let mut b = [0u8; 4];
    read(addr, &mut b).then(|| u32::from_le_bytes(b))
}

/// Writes committed, writable memory only.
pub fn write(addr: usize, data: &[u8]) -> bool {
    if addr == 0 {
        return false;
    }
    let mut a = addr;
    let end = addr + data.len();
    while a < end {
        let Some(m) = query(a) else { return false };
        if m.State != MEM_COMMIT || !writable(m.Protect) {
            return false;
        }
        a = m.BaseAddress + m.RegionSize;
    }
    let mut n = 0usize;
    // SAFETY: checked writable above; the kernel checks again.
    unsafe { WriteProcessMemory(GetCurrentProcess(), addr as *mut c_void, data.as_ptr() as *const c_void, data.len(), &mut n) != 0 && n == data.len() }
}

/// Writes code or read-only data: protection lifted for the write, instruction cache flushed.
pub fn patch(addr: usize, data: &[u8]) -> bool {
    let Some(m) = query(addr) else { return false };
    if m.State != MEM_COMMIT {
        return false;
    }
    let mut old = 0u32;
    // SAFETY: committed memory; protection restored below.
    unsafe {
        if VirtualProtect(addr as *mut c_void, data.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), addr as *mut u8, data.len());
        VirtualProtect(addr as *mut c_void, data.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, data.len());
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Modules

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub start: usize,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub base: usize,
    pub size: usize,
    pub sections: Vec<Section>,
    /// in the Windows loader's list (false: an image mapped by other means)
    pub listed: bool,
    pub is_exe: bool,
    pub timestamp: u32,
}

impl Module {
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// An exported symbol's address.
    pub fn export(&self, symbol: &str) -> Option<usize> {
        let pe = self.base + read_u32(self.base + 0x3C)? as usize;
        let dir = read_u32(pe + 24 + 112)? as usize;
        if dir == 0 {
            return None;
        }
        let e = self.base + dir;
        let n = read_u32(e + 24)? as usize;
        let funcs = self.base + read_u32(e + 28)? as usize;
        let names = self.base + read_u32(e + 32)? as usize;
        let ords = self.base + read_u32(e + 36)? as usize;
        for i in 0..n.min(1 << 20) {
            let np = self.base + read_u32(names + i * 4)? as usize;
            if cstr_at(np, 256).as_deref() == Some(symbol) {
                let mut o = [0u8; 2];
                read(ords + i * 2, &mut o).then_some(())?;
                let rva = read_u32(funcs + u16::from_le_bytes(o) as usize * 4)? as usize;
                return Some(self.base + rva);
            }
        }
        None
    }
}

fn cstr_at(addr: usize, max: usize) -> Option<String> {
    let mut b = vec![0u8; max];
    // read what is readable (a name near the end of a page)
    let mut n = max;
    while n > 0 && !read(addr, &mut b[..n]) {
        n /= 2;
    }
    let end = b[..n].iter().position(|&c| c == 0)?;
    Some(String::from_utf8_lossy(&b[..end]).into_owned())
}

/// Parses the PE image mapped at `base` (None if there is none).
pub fn image_at(base: usize, name: Option<String>, listed: bool) -> Option<Module> {
    let mut h = [0u8; 2];
    if !read(base, &mut h) || &h != b"MZ" {
        return None;
    }
    let pe = base + read_u32(base + 0x3C)? as usize;
    if read_u32(pe)? != 0x4550 {
        return None;
    }
    let mut c = [0u8; 4];
    read(pe + 4, &mut c).then_some(())?;
    let nsec = u16::from_le_bytes([c[2], c[3]]) as usize;
    let mut o = [0u8; 4];
    read(pe + 20, &mut o).then_some(())?;
    let opt_size = u16::from_le_bytes([o[0], o[1]]) as usize;
    let chars = u16::from_le_bytes([o[2], o[3]]);
    let timestamp = read_u32(pe + 8)?;
    let opt = pe + 24;
    let mut magic = [0u8; 2];
    read(opt, &mut magic).then_some(())?;
    if u16::from_le_bytes(magic) != 0x20B {
        return None;
    }
    let size = read_u32(opt + 56)? as usize;
    let mut sections = Vec::new();
    for i in 0..nsec.min(96) {
        let s = opt + opt_size + i * 40;
        let mut raw = [0u8; 8];
        read(s, &mut raw).then_some(())?;
        let n = raw.iter().position(|&c| c == 0).unwrap_or(8);
        sections.push(Section { name: String::from_utf8_lossy(&raw[..n]).into_owned(), size: read_u32(s + 8)? as usize, start: base + read_u32(s + 12)? as usize });
    }
    let mut m = Module { name: String::new(), base, size, sections, listed, is_exe: chars & 0x2000 == 0, timestamp };
    m.name = match name {
        Some(n) => n,
        None => {
            // the export directory's own name, else a made-up one
            let dir = read_u32(opt + 112).unwrap_or(0) as usize;
            let en = if dir != 0 { read_u32(base + dir + 12).and_then(|r| cstr_at(base + r as usize, 128)) } else { None };
            en.filter(|s| !s.is_empty()).unwrap_or_else(|| format!("image@{base:x}"))
        }
    };
    Some(m)
}

/// The Windows loader's modules.
pub fn listed_modules() -> Vec<Module> {
    let mut hs = vec![std::ptr::null_mut::<c_void>(); 1024];
    let mut need = 0u32;
    // SAFETY: buffers sized as passed.
    unsafe {
        if K32EnumProcessModules(GetCurrentProcess(), hs.as_mut_ptr(), (hs.len() * 8) as u32, &mut need) == 0 {
            return Vec::new();
        }
    }
    hs.truncate((need as usize / 8).min(1024));
    let mut out = Vec::new();
    for h in hs {
        let mut name = [0u16; 260];
        // SAFETY: out buffer.
        unsafe { K32GetModuleBaseNameW(GetCurrentProcess(), h, name.as_mut_ptr(), 260) };
        if let Some(m) = image_at(h as usize, Some(from_wide(&name)), true) {
            out.push(m);
        }
    }
    out
}

/// PE images the loader does not list (mapped by a runtime's own loader, for example): every
/// allocation that starts with a valid PE header.
pub fn unlisted_images(listed: &[Module]) -> Vec<Module> {
    let mut out = Vec::new();
    let mut a = 0x10000usize;
    while a < 0x7FFF_FFFE_0000 {
        let Some(m) = query(a) else { break };
        let next = m.BaseAddress + m.RegionSize.max(0x1000);
        if m.State == MEM_COMMIT && m.BaseAddress == m.AllocationBase && readable(m.Protect) && !listed.iter().any(|l| l.base == m.BaseAddress) {
            if let Some(img) = image_at(m.BaseAddress, None, false) {
                if img.size >= 0x1000 {
                    out.push(img);
                }
            }
        }
        if next <= a {
            break;
        }
        a = next;
    }
    out
}

/// Every module: listed ones first, then unlisted images.
pub fn modules() -> Vec<Module> {
    let mut v = listed_modules();
    let u = unlisted_images(&v);
    v.extend(u);
    v
}

// ---------------------------------------------------------------------------------------------
// Patterns

/// `"48 8B ?? 05"` → bytes with wildcards.
pub fn parse_pattern(p: &str) -> Result<Vec<Option<u8>>, String> {
    let v: Result<Vec<_>, _> = p.split_whitespace().map(|t| if t == "?" || t == "??" { Ok(None) } else { u8::from_str_radix(t, 16).map(Some).map_err(|_| format!("bad pattern byte '{t}'")) }).collect();
    let v = v?;
    if v.is_empty() || v.iter().all(|b| b.is_none()) {
        return Err("a pattern needs at least one fixed byte".into());
    }
    Ok(v)
}

/// Matches of `pat` in `[start, start+size)` (read in chunks; unreadable pages skipped).
pub fn scan_range(start: usize, size: usize, pat: &[Option<u8>], max: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let anchor = pat.iter().position(|b| b.is_some()).unwrap();
    let a_byte = pat[anchor].unwrap();
    const CHUNK: usize = 1 << 20;
    let mut off = 0;
    let mut buf = vec![0u8; CHUNK + pat.len()];
    while off < size && out.len() < max {
        let n = (size - off).min(CHUNK + pat.len() - 1);
        let b = &mut buf[..n];
        if !read(start + off, b) {
            // skip to the next page
            off = ((start + off + 0x1000) & !0xFFF) - start;
            continue;
        }
        let mut i = 0;
        while i + pat.len() <= n {
            match b[i + anchor..n - (pat.len() - anchor - 1)].iter().position(|&c| c == a_byte) {
                None => break,
                Some(k) => i += k,
            }
            if i + pat.len() > n {
                break;
            }
            if pat.iter().enumerate().all(|(k, p)| p.map(|x| b[i + k] == x).unwrap_or(true)) && i < CHUNK.min(n) {
                out.push(start + off + i);
                if out.len() >= max {
                    break;
                }
            }
            i += 1;
        }
        off += CHUNK;
    }
    out
}

/// Matches in a module (one section or every section).
pub fn scan_module(m: &Module, section: Option<&str>, pat: &[Option<u8>], max: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for s in &m.sections {
        if section.map(|n| n != s.name).unwrap_or(false) {
            continue;
        }
        out.extend(scan_range(s.start, s.size, pat, max - out.len()));
        if out.len() >= max {
            break;
        }
    }
    out
}

/// `insn + len + *(i32*)(insn + disp)`
pub fn rip_target(insn: usize, disp: usize, len: usize) -> Option<usize> {
    let d = read_u32(insn + disp)? as i32 as i64;
    Some((insn as i64 + len as i64 + d) as usize)
}

/// `a = base; for each offset: a = *(u64*)a + offset`
pub fn resolve_chain(base: usize, offsets: &[i64]) -> Option<usize> {
    let mut a = base;
    for o in offsets {
        let p = read_u64(a)? as usize;
        if p == 0 {
            return None;
        }
        a = (p as i64 + o) as usize;
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_patterns_and_modules() {
        let mut data = vec![b'.'; 64];
        data[4..20].copy_from_slice(b"SHK-PATTERN-TEST");
        let a = data.as_ptr() as usize;
        let mut b = [0u8; 4];
        assert!(read(a + 4, &mut b));
        assert_eq!(&b, b"SHK-");
        assert!(!read(0x10, &mut b));
        assert!(!write(0x10, &b));
        let pat = parse_pattern("53 48 4B 2D ?? 41 54").unwrap();
        assert_eq!(scan_range(a, 64, &pat, 4), vec![a + 4]);
        assert!(parse_pattern("?? ??").is_err());
        let ms = modules();
        assert!(ms.iter().any(|m| m.name.eq_ignore_ascii_case("kernel32.dll") && m.listed));
        let k = ms.iter().find(|m| m.name.eq_ignore_ascii_case("kernel32.dll")).unwrap();
        assert!(k.section(".text").is_some());
        let lla = k.export("LoadLibraryW").unwrap();
        assert_eq!(lla, unsafe { GetProcAddress(k.base as _, c"LoadLibraryW".as_ptr() as *const u8) } as usize);
        // a chain through a heap pointer
        let target = Box::new(0x1234u64);
        let holder = Box::new(&*target as *const u64 as u64);
        assert_eq!(resolve_chain(&*holder as *const u64 as usize, &[0]), Some(&*target as *const u64 as usize));
        // rip target of `lea rcx, [rip+0x10]`
        let code = [0x48u8, 0x8D, 0x0D, 0x10, 0, 0, 0];
        assert_eq!(rip_target(code.as_ptr() as usize, 3, 7), Some(code.as_ptr() as usize + 7 + 0x10));
    }
}
