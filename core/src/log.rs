//! The hook's log: `scripthook.log` next to the DLL (or `SHK_LOG`), and the debugger output.

use std::io::Write;
use std::sync::Mutex;

static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static START: Mutex<Option<f64>> = Mutex::new(None);

pub fn open(path: &std::path::Path) {
    if let Ok(f) = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(path) {
        *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
}

fn line(level: &str, msg: &str) {
    let now = shk_sys::now();
    let t0 = *START.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(now);
    let l = format!("[{:9.4}] {level:5} {msg}\n", now - t0);
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = f.write_all(l.as_bytes());
        let _ = f.flush();
    }
    let c = format!("scripthook: {msg}\0");
    // SAFETY: NUL-terminated.
    unsafe { shk_sys::OutputDebugStringA(c.as_ptr()) };
}

pub fn info(m: &str) {
    line("info", m)
}
pub fn warn(m: &str) {
    line("warn", m)
}
pub fn error(m: &str) {
    line("error", m)
}
pub fn debug(m: &str) {
    line("debug", m)
}
