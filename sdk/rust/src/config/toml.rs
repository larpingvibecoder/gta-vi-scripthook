//! A TOML 1.0 subset: comments, `[table]`, `[[array of tables]]`, dotted keys, bare / quoted
//! keys, basic / literal / multi-line strings, integers (decimal, hex, octal, binary, `_`),
//! floats (incl. `inf` / `nan`), booleans, arrays (multi-line, trailing commas), inline tables.
//! Dates are read as strings.

use super::Value;

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0, line: 1 };
    let mut root = Value::Table(Vec::new());
    // the table statements go into
    let mut cur: Vec<String> = Vec::new();
    let mut cur_array = false;
    loop {
        p.skip_ws_nl_comments();
        if p.eof() {
            break;
        }
        if p.peek() == b'[' {
            let arr = p.s.get(p.i + 1) == Some(&b'[');
            p.i += if arr { 2 } else { 1 };
            p.skip_ws();
            let key = p.key()?;
            p.skip_ws();
            p.expect(b']')?;
            if arr {
                p.expect(b']')?;
            }
            p.end_of_line()?;
            let t = walk(&mut root, &key, true).map_err(|e| p.err(&e))?;
            if arr {
                match t {
                    Value::Array(a) => a.push(Value::Table(Vec::new())),
                    Value::Table(tt) if tt.is_empty() => *t = Value::Array(vec![Value::Table(Vec::new())]),
                    _ => return Err(p.err(&format!("{} is not an array of tables", key.join(".")))),
                }
            } else if !matches!(t, Value::Table(_)) {
                return Err(p.err(&format!("{} is not a table", key.join("."))));
            }
            cur = key;
            cur_array = arr;
            continue;
        }
        let key = p.key()?;
        p.skip_ws();
        p.expect(b'=')?;
        p.skip_ws();
        let v = p.value()?;
        p.end_of_line()?;
        let mut t = walk(&mut root, &cur, false).map_err(|e| p.err(&e))?;
        if cur_array {
            if let Value::Array(a) = t {
                t = a.last_mut().ok_or_else(|| p.err("empty array of tables"))?;
            }
        }
        let (last, parents) = key.split_last().unwrap();
        let t = walk(t, parents, true).map_err(|e| p.err(&e))?;
        set(t, last, v).map_err(|e| p.err(&e))?;
    }
    Ok(root)
}

/// The table at `path` below `t` (created when missing; through the last element of arrays of
/// tables).
fn walk<'a>(mut t: &'a mut Value, path: &[String], create: bool) -> Result<&'a mut Value, String> {
    for k in path {
        if let Value::Array(a) = t {
            t = a.last_mut().ok_or("empty array")?;
        }
        let Value::Table(tt) = t else { return Err(format!("{k}: not a table")) };
        let pos = match tt.iter().position(|(kk, _)| kk == k) {
            Some(p) => p,
            None if create => {
                tt.push((k.clone(), Value::Table(Vec::new())));
                tt.len() - 1
            }
            None => return Err(format!("{k}: no such table")),
        };
        t = &mut tt[pos].1;
    }
    Ok(t)
}

fn set(t: &mut Value, k: &str, v: Value) -> Result<(), String> {
    let t = match t {
        Value::Array(a) => a.last_mut().ok_or("empty array")?,
        x => x,
    };
    let Value::Table(tt) = t else { return Err(format!("{k}: not a table")) };
    if tt.iter().any(|(kk, _)| kk == k) {
        return Err(format!("duplicate key {k}"));
    }
    tt.push((k.to_string(), v));
    Ok(())
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    line: usize,
}

impl Parser<'_> {
    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }
    fn peek(&self) -> u8 {
        self.s.get(self.i).copied().unwrap_or(0)
    }
    fn err(&self, m: &str) -> String {
        format!("line {}: {m}", self.line)
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), b' ' | b'\t') {
            self.i += 1;
        }
    }
    fn skip_ws_nl_comments(&mut self) {
        loop {
            match self.peek() {
                b' ' | b'\t' | b'\r' => self.i += 1,
                b'\n' => {
                    self.line += 1;
                    self.i += 1
                }
                b'#' => {
                    while !self.eof() && self.peek() != b'\n' {
                        self.i += 1;
                    }
                }
                _ => return,
            }
        }
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == c {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", c as char)))
        }
    }
    fn end_of_line(&mut self) -> Result<(), String> {
        self.skip_ws();
        if self.peek() == b'#' {
            while !self.eof() && self.peek() != b'\n' {
                self.i += 1;
            }
        }
        match self.peek() {
            0 | b'\n' | b'\r' => Ok(()),
            c => Err(self.err(&format!("unexpected '{}' after a value", c as char))),
        }
    }
    fn key(&mut self) -> Result<Vec<String>, String> {
        let mut parts = Vec::new();
        loop {
            self.skip_ws();
            let k = match self.peek() {
                b'"' => self.basic_string()?,
                b'\'' => self.literal_string()?,
                _ => {
                    let st = self.i;
                    while matches!(self.peek(), b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') {
                        self.i += 1;
                    }
                    if st == self.i {
                        return Err(self.err("expected a key"));
                    }
                    String::from_utf8_lossy(&self.s[st..self.i]).into_owned()
                }
            };
            parts.push(k);
            self.skip_ws();
            if self.peek() == b'.' {
                self.i += 1;
                continue;
            }
            return Ok(parts);
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            b'"' => {
                if self.s[self.i..].starts_with(b"\"\"\"") {
                    self.ml_basic()
                } else {
                    self.basic_string().map(Value::Str)
                }
            }
            b'\'' => {
                if self.s[self.i..].starts_with(b"'''") {
                    self.ml_literal()
                } else {
                    self.literal_string().map(Value::Str)
                }
            }
            b'[' => self.array(),
            b'{' => self.inline_table(),
            _ => self.scalar(),
        }
    }
    fn basic_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                0 | b'\n' => return Err(self.err("unterminated string")),
                b'"' => {
                    self.i += 1;
                    break;
                }
                b'\\' => {
                    self.i += 1;
                    self.escape(&mut out)?;
                }
                c => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
    fn escape(&mut self, out: &mut Vec<u8>) -> Result<(), String> {
        let c = self.peek();
        self.i += 1;
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'u' | b'U' => {
                let n = if c == b'u' { 4 } else { 8 };
                let h = std::str::from_utf8(self.s.get(self.i..self.i + n).ok_or_else(|| self.err("bad escape"))?).map_err(|_| self.err("bad escape"))?;
                let cp = u32::from_str_radix(h, 16).map_err(|_| self.err("bad escape"))?;
                self.i += n;
                let mut b = [0u8; 4];
                out.extend_from_slice(char::from_u32(cp).unwrap_or('?').encode_utf8(&mut b).as_bytes());
            }
            _ => return Err(self.err("bad escape")),
        }
        Ok(())
    }
    fn literal_string(&mut self) -> Result<String, String> {
        self.expect(b'\'')?;
        let st = self.i;
        while self.peek() != b'\'' {
            if matches!(self.peek(), 0 | b'\n') {
                return Err(self.err("unterminated string"));
            }
            self.i += 1;
        }
        let s = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
        self.i += 1;
        Ok(s)
    }
    fn ml_basic(&mut self) -> Result<Value, String> {
        self.i += 3;
        if self.peek() == b'\r' {
            self.i += 1;
        }
        if self.peek() == b'\n' {
            self.i += 1;
            self.line += 1;
        }
        let mut out = Vec::new();
        loop {
            if self.s[self.i..].starts_with(b"\"\"\"") {
                self.i += 3;
                break;
            }
            match self.peek() {
                0 => return Err(self.err("unterminated string")),
                b'\\' => {
                    self.i += 1;
                    if matches!(self.peek(), b'\n' | b'\r' | b' ' | b'\t') {
                        while matches!(self.peek(), b'\n' | b'\r' | b' ' | b'\t') {
                            if self.peek() == b'\n' {
                                self.line += 1;
                            }
                            self.i += 1;
                        }
                    } else {
                        self.escape(&mut out)?;
                    }
                }
                c => {
                    if c == b'\n' {
                        self.line += 1;
                    }
                    out.push(c);
                    self.i += 1;
                }
            }
        }
        Ok(Value::Str(String::from_utf8_lossy(&out).into_owned()))
    }
    fn ml_literal(&mut self) -> Result<Value, String> {
        self.i += 3;
        if self.peek() == b'\r' {
            self.i += 1;
        }
        if self.peek() == b'\n' {
            self.i += 1;
        }
        let st = self.i;
        while !self.s[self.i..].starts_with(b"'''") {
            if self.eof() {
                return Err(self.err("unterminated string"));
            }
            if self.peek() == b'\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        let s = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
        self.i += 3;
        Ok(Value::Str(s))
    }
    fn array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut v = Vec::new();
        loop {
            self.skip_ws_nl_comments();
            if self.peek() == b']' {
                self.i += 1;
                return Ok(Value::Array(v));
            }
            v.push(self.value()?);
            self.skip_ws_nl_comments();
            match self.peek() {
                b',' => self.i += 1,
                b']' => {
                    self.i += 1;
                    return Ok(Value::Array(v));
                }
                _ => return Err(self.err("expected ',' or ']' in an array")),
            }
        }
    }
    fn inline_table(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut t = Value::Table(Vec::new());
        loop {
            self.skip_ws_nl_comments();
            if self.peek() == b'}' {
                self.i += 1;
                return Ok(t);
            }
            let key = self.key()?;
            self.skip_ws();
            self.expect(b'=')?;
            self.skip_ws();
            let v = self.value()?;
            let (last, parents) = key.split_last().unwrap();
            let tt = walk(&mut t, parents, true).map_err(|e| self.err(&e))?;
            set(tt, last, v).map_err(|e| self.err(&e))?;
            self.skip_ws_nl_comments();
            match self.peek() {
                b',' => self.i += 1,
                b'}' => {
                    self.i += 1;
                    return Ok(t);
                }
                _ => return Err(self.err("expected ',' or '}' in an inline table")),
            }
        }
    }
    fn scalar(&mut self) -> Result<Value, String> {
        let st = self.i;
        while !matches!(self.peek(), 0 | b',' | b']' | b'}' | b'\n' | b'\r' | b'#') {
            self.i += 1;
        }
        let raw = std::str::from_utf8(&self.s[st..self.i]).map_err(|_| self.err("bad value"))?.trim();
        match raw {
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            "inf" | "+inf" => return Ok(Value::Float(f64::INFINITY)),
            "-inf" => return Ok(Value::Float(f64::NEG_INFINITY)),
            "nan" | "+nan" | "-nan" => return Ok(Value::Float(f64::NAN)),
            "" => return Err(self.err("missing value")),
            _ => {}
        }
        if let Some(i) = super::parse_int(raw) {
            return Ok(Value::Int(i));
        }
        let f = raw.replace('_', "");
        if let Ok(x) = f.parse::<f64>() {
            return Ok(Value::Float(x));
        }
        // dates and times stay text
        if raw.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return Ok(Value::Str(raw.to_string()));
        }
        Err(self.err(&format!("bad value '{raw}'")))
    }
}

/// A string as a TOML basic string literal.
pub fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_subset() {
        let t = r#"
# comment
game = "Game.1"   # trailing
n = 0x1F
big = 1_000
f = -2.5
on = true
list = [1, 2,
  3, ]   # multi-line
inline = { kind = "buckets", rip = { disp = 3, len = 7 }, xs = ["a", 'b'] }
a.b = "dotted"
lit = 'C:\path'
ml = """
line1
line2"""

[vars]
player = "PLAYER::PLAYER_ID()"

[[feature]]
id = "one"
tick = ["A(\"x\")", "B"]

[[feature]]
id = "two"
slider = { min = 0, max = 5, step = 1 }

[feature.extra]
"#;
        let v = parse(t).unwrap();
        assert_eq!(v.str_of("game").as_deref(), Some("Game.1"));
        assert_eq!(v.i64_of("n"), Some(31));
        assert_eq!(v.i64_of("big"), Some(1000));
        assert_eq!(v.f64_of("f"), Some(-2.5));
        assert_eq!(v.bool_of("on"), Some(true));
        assert_eq!(v.get("list").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(v.path("inline.rip.len").and_then(|x| x.as_i64()), Some(7));
        assert_eq!(v.path("inline.xs").unwrap().as_array().unwrap()[1].as_str(), Some("b"));
        assert_eq!(v.path("a.b").and_then(|x| x.as_str()), Some("dotted"));
        assert_eq!(v.str_of("lit").as_deref(), Some("C:\\path"));
        assert_eq!(v.str_of("ml").as_deref(), Some("line1\nline2"));
        assert_eq!(v.path("vars.player").and_then(|x| x.as_str()), Some("PLAYER::PLAYER_ID()"));
        let f = v.get("feature").unwrap().as_array().unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].strings_of("tick"), vec!["A(\"x\")".to_string(), "B".to_string()]);
        assert_eq!(f[1].path("slider.max").and_then(|x| x.as_f64()), Some(5.0));
        assert!(f[1].get("extra").is_some());
        assert!(parse("a = 1\na = 2").is_err());
        assert!(parse("a = ").is_err());
        assert_eq!(parse(&format!("s = {}", quote("a\"b\\c\n"))).unwrap().str_of("s").as_deref(), Some("a\"b\\c\n"));
    }
}
