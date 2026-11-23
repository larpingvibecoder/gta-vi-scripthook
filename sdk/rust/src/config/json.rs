//! A JSON reader (RFC 8259) producing [`Value`]; numbers without fraction or exponent are `Int`.

use super::Value;

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = P { s: text.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn err(&self, m: &str) -> String {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        format!("line {line}: {m}")
    }
    fn ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    fn lit(&mut self, w: &str, v: Value) -> Result<Value, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(self.err("bad literal"))
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 256 {
            return Err(self.err("nested too deeply"));
        }
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut t = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Table(t));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return Err(self.err("expected ':'"));
                    }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    t.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Table(t));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Array(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Array(a));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(_) => self.number(),
            None => Err(self.err("unexpected end")),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        if self.s.get(self.i) != Some(&b'"') {
            return Err(self.err("expected a string"));
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.s.get(self.i).ok_or_else(|| self.err("unterminated string"))?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *self.s.get(self.i).ok_or_else(|| self.err("bad escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            let mut b = [0u8; 4];
                            out.extend_from_slice(char::from_u32(cp).unwrap_or('?').encode_utf8(&mut b).as_bytes());
                        }
                        _ => return Err(self.err("bad escape")),
                    }
                }
                c => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).ok_or_else(|| self.err("bad \\u escape"))?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).map_err(|_| self.err("bad \\u escape"))?, 16).map_err(|_| self.err("bad \\u escape"))
    }
    fn number(&mut self) -> Result<Value, String> {
        let st = self.i;
        while matches!(self.s.get(self.i), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[st..self.i]).unwrap_or("");
        if t.is_empty() {
            return Err(self.err("unexpected character"));
        }
        if !t.contains(['.', 'e', 'E']) {
            if let Ok(i) = t.parse::<i64>() {
                return Ok(Value::Int(i));
            }
        }
        t.parse::<f64>().map(Value::Float).map_err(|_| self.err(&format!("bad number {t}")))
    }
}

/// Text as a JSON string literal.
pub fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses() {
        let v = super::parse(r#"{"NS": {"0x1234": {"name": "A", "params": [{"type": "int", "name": "x"}], "return_type": "void"}}, "n": -1.5e2, "u": "é😀", "z": null, "e": []}"#).unwrap();
        assert_eq!(v.path("NS.0x1234.name").and_then(|x| x.as_str()), Some("A"));
        let esc = super::parse("\"\\u00e9\\ud83d\\ude00\"").unwrap();
        assert_eq!(esc.as_str(), Some("\u{e9}\u{1F600}"));
        let ns = v.get("NS").unwrap();
        assert_eq!(ns.get("0x1234").unwrap().str_of("name").as_deref(), Some("A"));
        assert_eq!(v.f64_of("n"), Some(-150.0));
        assert_eq!(v.str_of("u").as_deref(), Some("\u{e9}\u{1F600}"));
        assert!(super::parse("[1,]").is_err());
        assert!(super::parse("{} x").is_err());
    }
}
