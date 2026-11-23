//! Reading the owner's executable (PE32+): sections, exports, byte patterns. Used to check that
//! the `@native_table` / `@script_tick` locations of a natives database match exactly one place
//! in the executable before the host is pointed at it. Read-only; nothing is written.

pub struct Section {
    pub name: String,
    pub va: u32,
    pub vsize: u32,
    pub raw_off: u32,
    pub raw_size: u32,
}

pub struct Pe {
    pub data: Vec<u8>,
    pub image_base: u64,
    pub sections: Vec<Section>,
    export_dir: (u32, u32),
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn u64_at(d: &[u8], o: usize) -> Option<u64> {
    d.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

impl Pe {
    pub fn parse(data: Vec<u8>) -> Result<Pe, String> {
        if data.get(..2) != Some(b"MZ") {
            return Err("not a PE file (no MZ)".into());
        }
        let pe = u32_at(&data, 0x3C).ok_or("truncated")? as usize;
        if data.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err("not a PE file (no PE signature)".into());
        }
        let nsec = u16_at(&data, pe + 6).ok_or("truncated")? as usize;
        let opt_size = u16_at(&data, pe + 20).ok_or("truncated")? as usize;
        let opt = pe + 24;
        if u16_at(&data, opt) != Some(0x20B) {
            return Err("not a 64-bit image (PE32+)".into());
        }
        let image_base = u64_at(&data, opt + 24).ok_or("truncated")?;
        let ndirs = u32_at(&data, opt + 108).unwrap_or(0);
        let export_dir = if ndirs > 0 { (u32_at(&data, opt + 112).unwrap_or(0), u32_at(&data, opt + 116).unwrap_or(0)) } else { (0, 0) };
        let mut sections = Vec::new();
        let st = opt + opt_size;
        for i in 0..nsec {
            let o = st + i * 40;
            let raw = data.get(o..o + 8).ok_or("truncated section table")?;
            let n = raw.iter().position(|&c| c == 0).unwrap_or(8);
            sections.push(Section {
                name: String::from_utf8_lossy(&raw[..n]).into_owned(),
                vsize: u32_at(&data, o + 8).unwrap_or(0),
                va: u32_at(&data, o + 12).unwrap_or(0),
                raw_size: u32_at(&data, o + 16).unwrap_or(0),
                raw_off: u32_at(&data, o + 20).unwrap_or(0),
            });
        }
        Ok(Pe { data, image_base, sections, export_dir })
    }

    /// File offset of an RVA.
    pub fn offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find(|s| rva >= s.va && rva < s.va + s.raw_size.max(s.vsize)).and_then(|s| {
            let k = rva - s.va;
            (k < s.raw_size).then(|| (s.raw_off + k) as usize)
        })
    }

    pub fn read_u32(&self, rva: u32) -> Option<u32> {
        u32_at(&self.data, self.offset(rva)?)
    }

    /// Exported symbol → RVA.
    pub fn export(&self, name: &str) -> Option<u32> {
        let (rva, _) = self.export_dir;
        if rva == 0 {
            return None;
        }
        let o = self.offset(rva)?;
        let nnames = u32_at(&self.data, o + 24)? as usize;
        let funcs = self.offset(u32_at(&self.data, o + 28)?)?;
        let names = self.offset(u32_at(&self.data, o + 32)?)?;
        let ords = self.offset(u32_at(&self.data, o + 36)?)?;
        for i in 0..nnames {
            let no = self.offset(u32_at(&self.data, names + i * 4)?)?;
            let end = self.data[no..].iter().position(|&c| c == 0)?;
            if &self.data[no..no + end] == name.as_bytes() {
                let ord = u16_at(&self.data, ords + i * 2)? as usize;
                return u32_at(&self.data, funcs + ord * 4);
            }
        }
        None
    }

    /// RVAs of every match of `pattern` ("48 8B ?? .."), in one section or all.
    pub fn scan(&self, pattern: &str, section: Option<&str>, max: usize) -> Result<Vec<u32>, String> {
        let pat = parse_pattern(pattern)?;
        if pat.is_empty() {
            return Err("empty pattern".into());
        }
        let mut out = Vec::new();
        for s in &self.sections {
            if section.map(|n| n != s.name).unwrap_or(false) {
                continue;
            }
            let start = s.raw_off as usize;
            let end = (start + s.raw_size as usize).min(self.data.len());
            if start >= end {
                continue;
            }
            let hay = &self.data[start..end];
            // first fixed byte as an anchor
            let anchor = pat.iter().position(|b| b.is_some());
            let mut i = 0;
            while i + pat.len() <= hay.len() {
                if let Some(a) = anchor {
                    match hay[i + a..hay.len() - (pat.len() - a - 1)].iter().position(|&c| Some(c) == pat[a]) {
                        Some(k) => i += k,
                        None => break,
                    }
                }
                if pat.iter().enumerate().all(|(k, b)| b.map(|b| hay[i + k] == b).unwrap_or(true)) {
                    out.push(s.va + i as u32);
                    if out.len() >= max {
                        return Ok(out);
                    }
                }
                i += 1;
            }
        }
        Ok(out)
    }
}

pub fn parse_pattern(p: &str) -> Result<Vec<Option<u8>>, String> {
    p.split_whitespace()
        .map(|t| if t == "?" || t == "??" { Ok(None) } else { u8::from_str_radix(t, 16).map(Some).map_err(|_| format!("bad pattern byte '{t}'")) })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn patterns() {
        assert_eq!(super::parse_pattern("48 ?? 05").unwrap(), vec![Some(0x48), None, Some(5)]);
        assert!(super::parse_pattern("4G").is_err());
    }
}
