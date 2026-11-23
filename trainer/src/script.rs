//! The feature statement language (docs/trainer.md): one statement per string, `;` separates
//! several; the body of `if` / `each` / `repeat` is the rest of the string.
//!
//! ```text
//! PLAYER::SET_PLAYER_INVINCIBLE($player, true)       native call (NS::NAME, NAME or 0xHASH)
//! $m = 0; STATS::STAT_GET_INT($stat, &$m, -1)        variables, &$var out parameters
//! wait 20                                           yield the script for 20 ms
//! waitfor STREAMING::HAS_MODEL_LOADED($hash), 5000  poll each tick until true or timeout ($ok)
//! if !$veh: fail("not in a vehicle")                conditions (rest of the string is the body)
//! each weapons: WEAPON::GIVE_WEAPON_TO_PED($ped, $hash, 9999, false, false)
//! repeat 25: ...; break                             loops ($i), break, stop
//! ```
//!
//! Expressions: numbers (decimal, `0x` hex), "text", true / false, `$var`, `.x .y .z`,
//! `! -` (unary), `* /`, `+ -` (text concatenates), comparisons, `&& ||`, parentheses, calls.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Vec3([f64; 3]),
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::Vec3(v) => v.iter().any(|x| *x != 0.0),
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(b) => Some(*b as i32 as f64),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::Bool(b) => Some(*b as i64),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn text(&self) -> String {
        match self {
            Value::Nil => "nil".into(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => fmt_f(*f),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.clone(),
            Value::Vec3(v) => format!("({:.2}, {:.2}, {:.2})", v[0], v[1], v[2]),
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "text",
            Value::Vec3(_) => "vector",
        }
    }
}

fn fmt_f(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        let s = format!("{f:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ---- syntax --------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(Value),
    Str(String),
    Var(String),
    /// `&$var` (out parameter; only as a call argument)
    Ref(String),
    Call(String, Vec<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Field(Box<Expr>, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Assign(String, Expr),
    Expr(Expr),
    Wait(Expr),
    WaitFor(Expr, Expr),
    If(Expr, Vec<Stmt>),
    Each(String, Vec<Stmt>),
    Repeat(Expr, Vec<Stmt>),
    Break,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Var(String),
    Num(Value),
    Hex(String),
    Str(String),
    Sym(&'static str),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    const SYMS: [&str; 22] = ["::", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "!", "<", ">", "(", ")", ",", ";", ":", ".", "&", "="];
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            let mut t = String::new();
            i += 1;
            loop {
                match b.get(i) {
                    None => return Err("unterminated text".into()),
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        match b.get(i + 1) {
                            Some('n') => t.push('\n'),
                            Some(&x) => t.push(x),
                            None => return Err("unterminated text".into()),
                        }
                        i += 2;
                    }
                    Some(&x) => {
                        t.push(x);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(t));
            continue;
        }
        if c == '$' {
            let st = i + 1;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            if st == i {
                return Err("'$' without a name".into());
            }
            out.push(Tok::Var(b[st..i].iter().collect()));
            continue;
        }
        if c.is_ascii_digit() {
            let st = i;
            if c == '0' && matches!(b.get(i + 1), Some('x' | 'X')) {
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                out.push(Tok::Hex(b[st..i].iter().collect()));
                continue;
            }
            let mut float = false;
            while i < b.len() && (b[i].is_ascii_digit() || (b[i] == '.' && !float && b.get(i + 1).map(|d| d.is_ascii_digit()).unwrap_or(false))) {
                if b[i] == '.' {
                    float = true;
                }
                i += 1;
            }
            let t: String = b[st..i].iter().collect();
            out.push(Tok::Num(if float { Value::Float(t.parse().map_err(|_| format!("bad number {t}"))?) } else { Value::Int(t.parse().map_err(|_| format!("bad number {t}"))?) }));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let st = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(b[st..i].iter().collect()));
            continue;
        }
        let rest: String = b[i..(i + 2).min(b.len())].iter().collect();
        match SYMS.iter().find(|s| rest.starts_with(**s)) {
            Some(s) => {
                out.push(Tok::Sym(s));
                i += s.chars().count();
            }
            None => return Err(format!("unexpected '{c}'")),
        }
    }
    Ok(out)
}

struct P {
    t: Vec<Tok>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn sym(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.sym(s) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(format!("expected '{s}'{}", self.near()))
        }
    }
    fn near(&self) -> String {
        match self.peek() {
            None => " at the end".into(),
            Some(t) => format!(" before {}", tok_text(t)),
        }
    }

    fn stmts(&mut self) -> Result<Vec<Stmt>, String> {
        let mut v = Vec::new();
        while self.peek().is_some() {
            if self.eat(";") {
                continue;
            }
            let (s, consumed_rest) = self.stmt()?;
            v.push(s);
            if consumed_rest {
                break;
            }
            if self.peek().is_some() && !self.eat(";") {
                return Err(format!("expected ';'{}", self.near()));
            }
        }
        Ok(v)
    }

    /// (statement, true when it took the rest of the string as its body)
    fn stmt(&mut self) -> Result<(Stmt, bool), String> {
        if let Some(Tok::Ident(k)) = self.peek().cloned() {
            match k.as_str() {
                "if" => {
                    self.i += 1;
                    let c = self.expr()?;
                    self.expect(":")?;
                    return Ok((Stmt::If(c, self.stmts()?), true));
                }
                "each" => {
                    self.i += 1;
                    let Some(Tok::Ident(l)) = self.peek().cloned() else { return Err(format!("each: expected a list name{}", self.near())) };
                    self.i += 1;
                    self.expect(":")?;
                    return Ok((Stmt::Each(l, self.stmts()?), true));
                }
                "repeat" => {
                    self.i += 1;
                    let n = self.expr()?;
                    self.expect(":")?;
                    return Ok((Stmt::Repeat(n, self.stmts()?), true));
                }
                "wait" if !matches!(self.t.get(self.i + 1), Some(Tok::Sym("("))) => {
                    self.i += 1;
                    return Ok((Stmt::Wait(self.expr()?), false));
                }
                "waitfor" => {
                    self.i += 1;
                    let c = self.expr()?;
                    self.expect(",")?;
                    return Ok((Stmt::WaitFor(c, self.expr()?), false));
                }
                "break" => {
                    self.i += 1;
                    return Ok((Stmt::Break, false));
                }
                "stop" => {
                    self.i += 1;
                    return Ok((Stmt::Stop, false));
                }
                _ => {}
            }
        }
        if let (Some(Tok::Var(v)), Some(Tok::Sym("="))) = (self.peek().cloned(), self.t.get(self.i + 1)) {
            self.i += 2;
            return Ok((Stmt::Assign(v, self.expr()?), false));
        }
        Ok((Stmt::Expr(self.expr()?), false))
    }

    fn expr(&mut self) -> Result<Expr, String> {
        self.bin(0)
    }

    fn bin(&mut self, level: usize) -> Result<Expr, String> {
        const LEVELS: [&[(&str, Op)]; 5] = [&[("||", Op::Or)], &[("&&", Op::And)], &[("==", Op::Eq), ("!=", Op::Ne), ("<=", Op::Le), (">=", Op::Ge), ("<", Op::Lt), (">", Op::Gt)], &[("+", Op::Add), ("-", Op::Sub)], &[("*", Op::Mul), ("/", Op::Div)]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.bin(level + 1)?;
        'outer: loop {
            for (s, op) in LEVELS[level] {
                if self.eat(s) {
                    let r = self.bin(level + 1)?;
                    l = Expr::Bin(*op, Box::new(l), Box::new(r));
                    continue 'outer;
                }
            }
            return Ok(l);
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        let mut e = self.primary()?;
        while self.eat(".") {
            let Some(Tok::Ident(f)) = self.peek().cloned() else { return Err(format!("expected x, y or z after '.'{}", self.near())) };
            self.i += 1;
            let k = match f.as_str() {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                o => return Err(format!("unknown field .{o} (x, y or z)")),
            };
            e = Expr::Field(Box::new(e), k);
        }
        Ok(e)
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect("(")?;
        let mut a = Vec::new();
        if self.eat(")") {
            return Ok(a);
        }
        loop {
            if self.eat("&") {
                let Some(Tok::Var(v)) = self.peek().cloned() else { return Err(format!("expected $var after '&'{}", self.near())) };
                self.i += 1;
                a.push(Expr::Ref(v));
            } else {
                a.push(self.expr()?);
            }
            if self.eat(")") {
                return Ok(a);
            }
            self.expect(",")?;
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.i += 1;
                Ok(Expr::Num(v))
            }
            Some(Tok::Hex(h)) => {
                self.i += 1;
                if self.sym("(") {
                    let a = self.args()?;
                    return Ok(Expr::Call(h, a));
                }
                let v = u64::from_str_radix(&h[2..], 16).map_err(|_| format!("bad hex {h}"))?;
                Ok(Expr::Num(Value::Int(v as i64)))
            }
            Some(Tok::Str(s)) => {
                self.i += 1;
                Ok(Expr::Str(s))
            }
            Some(Tok::Var(v)) => {
                self.i += 1;
                Ok(Expr::Var(v))
            }
            Some(Tok::Ident(id)) => {
                self.i += 1;
                match id.as_str() {
                    "true" => return Ok(Expr::Num(Value::Bool(true))),
                    "false" => return Ok(Expr::Num(Value::Bool(false))),
                    _ => {}
                }
                let mut name = id;
                if self.eat("::") {
                    let Some(Tok::Ident(n2)) = self.peek().cloned() else { return Err(format!("expected a name after '::'{}", self.near())) };
                    self.i += 1;
                    name = format!("{name}::{n2}");
                }
                if !self.sym("(") {
                    return Err(format!("'{name}' is not a value (calls need parentheses)"));
                }
                let a = self.args()?;
                Ok(Expr::Call(name, a))
            }
            Some(Tok::Sym("(")) => {
                self.i += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(t) => Err(format!("unexpected {}", tok_text(&t))),
            None => Err("unexpected end".into()),
        }
    }
}

fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Ident(s) | Tok::Hex(s) => format!("'{s}'"),
        Tok::Var(v) => format!("'${v}'"),
        Tok::Num(v) => format!("'{}'", v.text()),
        Tok::Str(s) => format!("\"{s}\""),
        Tok::Sym(s) => format!("'{s}'"),
    }
}

/// Parses one statement string (which may hold several, `;`-separated).
pub fn parse(s: &str) -> Result<Vec<Stmt>, String> {
    let mut p = P { t: lex(s)?, i: 0 };
    p.stmts()
}

pub fn parse_expr(s: &str) -> Result<Expr, String> {
    let mut p = P { t: lex(s)?, i: 0 };
    let e = p.expr()?;
    if p.peek().is_some() {
        return Err(format!("unexpected text{}", p.near()));
    }
    Ok(e)
}

/// Builtin function names (everything else with parentheses is a native).
pub const BUILTINS: [&str; 18] = ["hash", "vec", "int", "float", "abs", "min", "max", "notify", "hud", "fly", "fps", "on", "value", "mem", "setmem", "save_location", "fail", "text"];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

// ---- analysis ------------------------------------------------------------------------------

/// What statements use.
#[derive(Debug, Default, Clone)]
pub struct Uses {
    pub natives: Vec<String>,
    pub lists: Vec<String>,
    pub vars: Vec<String>,
    pub mem: Vec<String>,
    /// builtins called (hud, fly, notify, ...)
    pub builtins: Vec<String>,
}

fn push(v: &mut Vec<String>, s: &str) {
    if !v.iter().any(|x| x == s) {
        v.push(s.to_string());
    }
}

pub fn uses_expr(e: &Expr, u: &mut Uses) {
    match e {
        Expr::Var(v) | Expr::Ref(v) => push(&mut u.vars, v),
        Expr::Call(n, a) => {
            if is_builtin(n) {
                push(&mut u.builtins, n);
                if matches!(n.as_str(), "mem" | "setmem") {
                    if let Some(Expr::Str(l)) = a.first() {
                        push(&mut u.mem, l);
                    }
                }
            } else {
                push(&mut u.natives, n);
            }
            for x in a {
                uses_expr(x, u);
            }
        }
        Expr::Not(x) | Expr::Neg(x) | Expr::Field(x, _) => uses_expr(x, u),
        Expr::Bin(_, a, b) => {
            uses_expr(a, u);
            uses_expr(b, u);
        }
        Expr::Num(_) | Expr::Str(_) => {}
    }
}

pub fn uses(stmts: &[Stmt], u: &mut Uses) {
    for s in stmts {
        match s {
            Stmt::Assign(_, e) | Stmt::Expr(e) | Stmt::Wait(e) => uses_expr(e, u),
            Stmt::WaitFor(a, b) => {
                uses_expr(a, u);
                uses_expr(b, u);
            }
            Stmt::If(c, b) | Stmt::Repeat(c, b) => {
                uses_expr(c, u);
                uses(b, u);
            }
            Stmt::Each(l, b) => {
                push(&mut u.lists, l);
                uses(b, u);
            }
            Stmt::Break | Stmt::Stop => {}
        }
    }
}

// ---- evaluation ----------------------------------------------------------------------------

/// A native-call argument.
#[derive(Debug, Clone, PartialEq)]
pub enum NArg {
    Val(Value),
    /// out parameter with the variable's current value
    Out(Value),
}

/// A list item (`each` and list features).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub name: String,
    pub label: String,
    pub hash: Option<u32>,
    pub pos: Option<[f64; 3]>,
    pub heading: Option<f64>,
}

impl Item {
    pub fn hash(&self) -> u32 {
        self.hash.unwrap_or_else(|| scripthook_sdk::joaat(&self.name))
    }
}

/// What statements reach outside the interpreter.
pub trait Env {
    /// (result, out values in order)
    fn native(&mut self, name: &str, args: &[NArg]) -> Result<(Value, Vec<Value>), String>;
    fn wait(&mut self, ms: u32) -> Result<(), String>;
    /// a builtin with side effects or outside state (notify, hud, fly, fps, on, value, mem,
    /// setmem, save_location); `None` = not handled
    fn builtin(&mut self, name: &str, args: &[Value]) -> Option<Result<Value, String>>;
    fn list(&mut self, name: &str) -> Option<Vec<Item>>;
}

/// Global variables (`[vars]`): evaluated at their first use in each tick.
#[derive(Default, Clone)]
pub struct Globals {
    pub defs: HashMap<String, Expr>,
    pub cache: HashMap<String, Value>,
}

impl Globals {
    pub fn new_tick(&mut self) {
        self.cache.clear();
    }
}

pub enum Flow {
    Next,
    Break,
    Stop,
}

/// Locals of one running statement list (a feature keeps its own between ticks).
#[derive(Default, Clone, Debug)]
pub struct Interp {
    pub locals: HashMap<String, Value>,
    depth: usize,
}

impl Interp {
    pub fn new() -> Interp {
        Interp::default()
    }

    pub fn set(&mut self, k: &str, v: Value) {
        self.locals.insert(k.to_string(), v);
    }

    pub fn set_item(&mut self, it: &Item, i: usize) {
        self.set("item", Value::Str(it.name.clone()));
        self.set("label", Value::Str(if it.label.is_empty() { it.name.clone() } else { it.label.clone() }));
        self.set("hash", Value::Int(it.hash() as i64));
        self.set("i", Value::Int(i as i64));
        match it.pos {
            Some(p) => {
                self.set("pos", Value::Vec3(p));
                self.set("x", Value::Float(p[0]));
                self.set("y", Value::Float(p[1]));
                self.set("z", Value::Float(p[2]));
            }
            None => {
                self.locals.remove("pos");
            }
        }
        match it.heading {
            Some(h) => self.set("heading", Value::Float(h)),
            None => self.set("heading", Value::Nil),
        }
    }

    pub fn run(&mut self, stmts: &[Stmt], g: &mut Globals, env: &mut dyn Env) -> Result<Flow, String> {
        for s in stmts {
            match self.stmt(s, g, env)? {
                Flow::Next => {}
                f => return Ok(f),
            }
        }
        Ok(Flow::Next)
    }

    fn stmt(&mut self, s: &Stmt, g: &mut Globals, env: &mut dyn Env) -> Result<Flow, String> {
        match s {
            Stmt::Assign(v, e) => {
                let x = self.eval(e, g, env)?;
                self.locals.insert(v.clone(), x);
            }
            Stmt::Expr(e) => {
                self.eval(e, g, env)?;
            }
            Stmt::Wait(e) => {
                let ms = self.eval(e, g, env)?.as_f64().ok_or("wait: a number of milliseconds")?;
                env.wait(ms.max(0.0) as u32)?;
                g.new_tick();
            }
            Stmt::WaitFor(c, t) => {
                let timeout = self.eval(t, g, env)?.as_f64().ok_or("waitfor: a timeout in milliseconds")?;
                let start = scripthook_sdk::win::now();
                let mut ok = false;
                loop {
                    if self.eval(c, g, env)?.truthy() {
                        ok = true;
                        break;
                    }
                    if (scripthook_sdk::win::now() - start) * 1000.0 >= timeout {
                        break;
                    }
                    env.wait(0)?;
                    g.new_tick();
                }
                self.set("ok", Value::Bool(ok));
            }
            Stmt::If(c, body) => {
                if self.eval(c, g, env)?.truthy() {
                    return self.run(body, g, env);
                }
            }
            Stmt::Each(l, body) => {
                let items = env.list(l).ok_or_else(|| format!("no list '{l}'"))?;
                for (i, it) in items.iter().enumerate() {
                    self.set_item(it, i);
                    match self.run(body, g, env)? {
                        Flow::Break => break,
                        Flow::Stop => return Ok(Flow::Stop),
                        Flow::Next => {}
                    }
                }
            }
            Stmt::Repeat(n, body) => {
                let n = self.eval(n, g, env)?.as_i64().ok_or("repeat: a count")?;
                for i in 0..n.max(0) {
                    self.set("i", Value::Int(i));
                    match self.run(body, g, env)? {
                        Flow::Break => break,
                        Flow::Stop => return Ok(Flow::Stop),
                        Flow::Next => {}
                    }
                }
            }
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Stop => return Ok(Flow::Stop),
        }
        Ok(Flow::Next)
    }

    fn var(&mut self, v: &str, g: &mut Globals, env: &mut dyn Env) -> Result<Value, String> {
        if let Some(x) = self.locals.get(v) {
            return Ok(x.clone());
        }
        if let Some(x) = g.cache.get(v) {
            return Ok(x.clone());
        }
        if let Some(e) = g.defs.get(v).cloned() {
            if self.depth > 16 {
                return Err(format!("${v}: variables refer to each other in a loop"));
            }
            self.depth += 1;
            let mut sub = Interp { locals: HashMap::new(), depth: self.depth };
            let r = sub.eval(&e, g, env);
            self.depth -= 1;
            let x = r.map_err(|e| format!("${v}: {e}"))?;
            g.cache.insert(v.to_string(), x.clone());
            return Ok(x);
        }
        Ok(Value::Nil)
    }

    pub fn eval(&mut self, e: &Expr, g: &mut Globals, env: &mut dyn Env) -> Result<Value, String> {
        Ok(match e {
            Expr::Num(v) => v.clone(),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Var(v) => self.var(v, g, env)?,
            Expr::Ref(v) => return Err(format!("&${v} is only allowed as a native's argument")),
            Expr::Not(x) => Value::Bool(!self.eval(x, g, env)?.truthy()),
            Expr::Neg(x) => match self.eval(x, g, env)? {
                Value::Int(i) => Value::Int(-i),
                Value::Float(f) => Value::Float(-f),
                Value::Vec3(v) => Value::Vec3([-v[0], -v[1], -v[2]]),
                o => return Err(format!("cannot negate {}", o.type_name())),
            },
            Expr::Field(x, k) => match self.eval(x, g, env)? {
                Value::Vec3(v) => Value::Float(v[*k]),
                o => return Err(format!(".{} of {} (not a vector)", ["x", "y", "z"][*k], o.type_name())),
            },
            Expr::Bin(Op::And, a, b) => Value::Bool(self.eval(a, g, env)?.truthy() && self.eval(b, g, env)?.truthy()),
            Expr::Bin(Op::Or, a, b) => Value::Bool(self.eval(a, g, env)?.truthy() || self.eval(b, g, env)?.truthy()),
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(a, g, env)?, self.eval(b, g, env)?);
                binop(*op, x, y)?
            }
            Expr::Call(name, args) => {
                if is_builtin(name) {
                    let mut vals = Vec::with_capacity(args.len());
                    for a in args {
                        vals.push(self.eval(a, g, env)?);
                    }
                    return builtin(name, &vals, env);
                }
                let mut nargs = Vec::with_capacity(args.len());
                let mut outs = Vec::new();
                for a in args {
                    match a {
                        Expr::Ref(v) => {
                            nargs.push(NArg::Out(self.var(v, g, env)?));
                            outs.push(v.clone());
                        }
                        x => nargs.push(NArg::Val(self.eval(x, g, env)?)),
                    }
                }
                let (r, out_vals) = env.native(name, &nargs)?;
                for (v, x) in outs.iter().zip(out_vals) {
                    self.locals.insert(v.clone(), x);
                }
                r
            }
        })
    }
}

fn num2(x: &Value, y: &Value) -> Option<(f64, f64, bool)> {
    let ints = matches!((x, y), (Value::Int(_) | Value::Bool(_), Value::Int(_) | Value::Bool(_)));
    Some((x.as_f64()?, y.as_f64()?, ints))
}

fn binop(op: Op, x: Value, y: Value) -> Result<Value, String> {
    use Value::*;
    Ok(match op {
        Op::Add => match (&x, &y) {
            (Str(a), b) => Str(format!("{a}{}", b.text())),
            (a, Str(b)) => Str(format!("{}{b}", a.text())),
            (Vec3(a), Vec3(b)) => Vec3([a[0] + b[0], a[1] + b[1], a[2] + b[2]]),
            _ => arith(op, &x, &y)?,
        },
        Op::Sub => match (&x, &y) {
            (Vec3(a), Vec3(b)) => Vec3([a[0] - b[0], a[1] - b[1], a[2] - b[2]]),
            _ => arith(op, &x, &y)?,
        },
        Op::Mul => match (&x, &y) {
            (Vec3(a), s) | (s, Vec3(a)) if s.as_f64().is_some() => {
                let k = s.as_f64().unwrap();
                Vec3([a[0] * k, a[1] * k, a[2] * k])
            }
            _ => arith(op, &x, &y)?,
        },
        Op::Div => arith(op, &x, &y)?,
        Op::Eq | Op::Ne => {
            let eq = match num2(&x, &y) {
                Some((a, b, _)) if !matches!((&x, &y), (Str(_), Str(_))) => a == b,
                _ => match (&x, &y) {
                    (Str(a), Str(b)) => a.eq_ignore_ascii_case(b),
                    _ => x == y,
                },
            };
            Bool(if op == Op::Eq { eq } else { !eq })
        }
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let (a, b, _) = num2(&x, &y).ok_or_else(|| format!("cannot compare {} and {}", x.type_name(), y.type_name()))?;
            Bool(match op {
                Op::Lt => a < b,
                Op::Le => a <= b,
                Op::Gt => a > b,
                _ => a >= b,
            })
        }
        Op::And | Op::Or => unreachable!(),
    })
}

fn arith(op: Op, x: &Value, y: &Value) -> Result<Value, String> {
    let (a, b, ints) = num2(x, y).ok_or_else(|| format!("cannot use {} and {} in arithmetic", x.type_name(), y.type_name()))?;
    Ok(match op {
        Op::Add if ints => Value::Int(a as i64 + b as i64),
        Op::Sub if ints => Value::Int(a as i64 - b as i64),
        Op::Mul if ints => Value::Int(a as i64 * b as i64),
        Op::Add => Value::Float(a + b),
        Op::Sub => Value::Float(a - b),
        Op::Mul => Value::Float(a * b),
        Op::Div => {
            if b == 0.0 {
                return Err("division by zero".into());
            }
            Value::Float(a / b)
        }
        _ => unreachable!(),
    })
}

fn builtin(name: &str, a: &[Value], env: &mut dyn Env) -> Result<Value, String> {
    let n = |k: usize| -> Result<f64, String> { a.get(k).and_then(|v| v.as_f64()).ok_or_else(|| format!("{name}: argument {} must be a number", k + 1)) };
    Ok(match name {
        "hash" => Value::Int(scripthook_sdk::joaat(&a.first().map(|v| v.text()).unwrap_or_default()) as i64),
        "vec" => match a {
            [Value::Vec3(v)] => Value::Vec3(*v),
            _ => Value::Vec3([n(0)?, n(1)?, n(2)?]),
        },
        "int" => Value::Int(n(0)?.trunc() as i64),
        "float" => Value::Float(n(0)?),
        "text" => Value::Str(a.iter().map(|v| v.text()).collect::<Vec<_>>().join("")),
        "abs" => match a.first() {
            Some(Value::Int(i)) => Value::Int(i.abs()),
            _ => Value::Float(n(0)?.abs()),
        },
        "min" | "max" => {
            let (x, y) = (n(0)?, n(1)?);
            let r = if name == "min" { x.min(y) } else { x.max(y) };
            if matches!((&a[0], &a[1]), (Value::Int(_), Value::Int(_))) {
                Value::Int(r as i64)
            } else {
                Value::Float(r)
            }
        }
        "fail" => return Err(a.first().map(|v| v.text()).unwrap_or_else(|| "failed".into())),
        _ => match env.builtin(name, a) {
            Some(r) => r?,
            None => return Err(format!("{name}() is not available here")),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake game: natives are Rust closures over a small world.
    #[derive(Default)]
    struct World {
        calls: Vec<String>,
        waits: u32,
        notes: Vec<String>,
        money: i64,
        loaded_after: u32,
    }

    impl Env for World {
        fn native(&mut self, name: &str, a: &[NArg]) -> Result<(Value, Vec<Value>), String> {
            self.calls.push(name.to_string());
            let v = |k: usize| match a.get(k) {
                Some(NArg::Val(x)) | Some(NArg::Out(x)) => x.clone(),
                None => Value::Nil,
            };
            Ok(match name {
                "PLAYER::PLAYER_ID" => (Value::Int(7), vec![]),
                "PLAYER::PLAYER_PED_ID" => (Value::Int(100), vec![]),
                "ENTITY::GET_ENTITY_COORDS" => (Value::Vec3([1.0, 2.0, 3.0]), vec![]),
                "STATS::STAT_GET_INT" => (Value::Bool(true), vec![Value::Int(self.money)]),
                "STATS::STAT_SET_INT" => {
                    self.money = v(1).as_i64().unwrap();
                    (Value::Bool(true), vec![])
                }
                "STREAMING::HAS_MODEL_LOADED" => {
                    self.loaded_after = self.loaded_after.saturating_sub(1);
                    (Value::Bool(self.loaded_after == 0), vec![])
                }
                "0x1234" => (Value::Int(v(0).as_i64().unwrap() * 2), vec![]),
                _ => (Value::Nil, vec![]),
            })
        }
        fn wait(&mut self, _ms: u32) -> Result<(), String> {
            self.waits += 1;
            Ok(())
        }
        fn builtin(&mut self, name: &str, a: &[Value]) -> Option<Result<Value, String>> {
            match name {
                "notify" => {
                    self.notes.push(a[0].text());
                    Some(Ok(Value::Nil))
                }
                _ => None,
            }
        }
        fn list(&mut self, name: &str) -> Option<Vec<Item>> {
            (name == "weapons").then(|| vec![Item { name: "WEAPON_A".into(), ..Default::default() }, Item { name: "WEAPON_B".into(), hash: Some(5), ..Default::default() }])
        }
    }

    fn run(src: &[&str], w: &mut World, g: &mut Globals) -> Result<Interp, String> {
        let mut it = Interp::new();
        it.set("value", Value::Int(5000));
        for s in src {
            it.run(&parse(s)?, g, w)?;
        }
        Ok(it)
    }

    #[test]
    fn parses_the_shipped_forms() {
        for s in [
            "PLAYER::SET_PLAYER_INVINCIBLE($player, true)",
            "ENTITY::SET_ENTITY_HEALTH($ped, ENTITY::GET_ENTITY_MAX_HEALTH($ped), 0)",
            "repeat 25: ENTITY::SET_ENTITY_COORDS_NO_OFFSET($e, $w.x, $w.y, 800.0 - $i * 32.0, false, false, false); wait 20; if MISC::GET_GROUND_Z_FOR_3D_COORD($w.x, $w.y, 800.0 - $i * 32.0, &$z, false, false): $found = true; break",
            "if $veh: ENTITY::SET_ENTITY_INVINCIBLE($veh, true); VEHICLE::SET_VEHICLE_CAN_BE_VISIBLY_DAMAGED($veh, false)",
            "waitfor STREAMING::HAS_MODEL_LOADED($hash), 5000",
            "hud(\"pos \" + ENTITY::GET_ENTITY_COORDS($ped, true) + \"  heading \" + ENTITY::GET_ENTITY_HEADING($ped))",
            "each weapons: WEAPON::GIVE_WEAPON_TO_PED($ped, $hash, 9999, false, false)",
            "if !$ok: fail(\"model \" + $label + \" did not load\")",
            "CAM::GET_GAMEPLAY_CAM_ROT(2).z",
            "0x1234(1)",
        ] {
            parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        }
        assert!(parse("PLAYER::X(").is_err());
        assert!(parse("$a = ").is_err());
        assert!(parse("PLAYER_ID").is_err(), "a call needs parentheses");
        let mut u = Uses::default();
        uses(&parse("each weapons: WEAPON::GIVE_WEAPON_TO_PED($ped, $hash, 1, false, false); setmem(\"money\", 5)").unwrap(), &mut u);
        assert_eq!(u.natives, vec!["WEAPON::GIVE_WEAPON_TO_PED"]);
        assert_eq!(u.lists, vec!["weapons"]);
        assert_eq!(u.mem, vec!["money"]);
        assert!(u.vars.contains(&"ped".to_string()));
    }

    #[test]
    fn evaluates() {
        let mut w = World { money: 12345, loaded_after: 3, ..Default::default() };
        let mut g = Globals::default();
        g.defs.insert("player".into(), parse_expr("PLAYER::PLAYER_ID()").unwrap());
        g.defs.insert("ped".into(), parse_expr("PLAYER::PLAYER_PED_ID()").unwrap());
        g.defs.insert("stat".into(), parse_expr("hash(\"SP0_TOTAL_CASH\")").unwrap());
        let it = run(&["$m = 0", "STATS::STAT_GET_INT($stat, &$m, -1)", "STATS::STAT_SET_INT($stat, $m + int($value), true)", "notify(\"money: \" + ($m + int($value)))"], &mut w, &mut g).unwrap();
        assert_eq!(w.money, 17345);
        assert_eq!(w.notes, vec!["money: 17345"]);
        assert_eq!(it.locals["m"], Value::Int(12345));
        // globals evaluated once per tick
        let n = w.calls.iter().filter(|c| *c == "PLAYER::PLAYER_ID").count();
        run(&["$a = $player + $player"], &mut w, &mut g).unwrap();
        assert_eq!(w.calls.iter().filter(|c| *c == "PLAYER::PLAYER_ID").count(), n + 1);
        // waitfor, $ok
        let it = run(&["waitfor STREAMING::HAS_MODEL_LOADED(1), 5000", "$r = $ok"], &mut w, &mut g).unwrap();
        assert_eq!(it.locals["r"], Value::Bool(true));
        assert!(w.waits >= 2);
        // loops, break, fields, text
        let it = run(&["$s = 0; repeat 10: $s = $s + $i; if $i == 4: break", "$v = ENTITY::GET_ENTITY_COORDS($ped, true)", "$t = \"pos \" + $v", "$z = $v.z * 2"], &mut w, &mut g).unwrap();
        assert_eq!(it.locals["s"], Value::Int(10));
        assert_eq!(it.locals["t"], Value::Str("pos (1.00, 2.00, 3.00)".into()));
        assert_eq!(it.locals["z"], Value::Float(6.0));
        let it = run(&["$h = 0; each weapons: $h = $h + $hash"], &mut w, &mut g).unwrap();
        assert_eq!(it.locals["h"], Value::Int(scripthook_sdk::joaat("WEAPON_A") as i64 + 5));
        // fail and stop
        let e = run(&["if !false: fail(\"not in a vehicle\")"], &mut w, &mut g).unwrap_err();
        assert_eq!(e, "not in a vehicle");
        let it = run(&["$a = 1; stop; $a = 2"], &mut w, &mut g).unwrap();
        assert_eq!(it.locals["a"], Value::Int(1));
        assert_eq!(run(&["$x = 0x1234(21)"], &mut w, &mut g).unwrap().locals["x"], Value::Int(42));
        assert!(run(&["$x = 1 / 0"], &mut w, &mut g).is_err());
        assert!(run(&["fly(1)"], &mut w, &mut g).unwrap_err().contains("not available"));
    }
}
