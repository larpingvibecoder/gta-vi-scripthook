//! Config readers: a TOML subset and JSON, both producing [`Value`].

pub mod json;
pub mod toml;

/// A parsed value. Tables keep their keys in file order.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<Value>),
    Table(Vec<(String, Value)>),
    Null,
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Table(t) => t.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// `a.b.c`
    pub fn path(&self, p: &str) -> Option<&Value> {
        p.split('.').try_fold(self, |v, k| v.get(k))
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            Value::Str(s) => parse_int(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_table(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }
    pub fn str_of(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str()).map(str::to_string)
    }
    pub fn f64_of(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.as_f64())
    }
    pub fn i64_of(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_i64())
    }
    pub fn bool_of(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.as_bool())
    }
    /// A string or an array of strings.
    pub fn strings_of(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::Str(s)) => vec![s.clone()],
            Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        }
    }
}

/// `123`, `-5`, `0x1F`, `1_000`.
pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim().replace('_', "");
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, t.trim_start_matches('+').to_string()),
    };
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()? as i64
    } else if let Some(o) = t.strip_prefix("0o") {
        i64::from_str_radix(o, 8).ok()?
    } else if let Some(b) = t.strip_prefix("0b") {
        i64::from_str_radix(b, 2).ok()?
    } else {
        t.parse::<i64>().ok()?
    };
    Some(if neg { -v } else { v })
}
