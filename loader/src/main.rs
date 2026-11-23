//! `shk-loader`: starts a program (or attaches to a running one) and loads `scripthook.dll` into
//! it with `LoadLibraryW` on a remote thread.
//!
//! For your own single-player game only. It loads a DLL into a process you started; it does not
//! bypass copy protection, anti-cheat, licence or integrity checks, and must not be used with
//! online games or services.

use shk_sys::*;
use std::ffi::c_void;
use std::path::PathBuf;

const USAGE: &str = "shk-loader: load scripthook.dll into your own single-player game process

usage:
  shk-loader --launch <program.exe> [--dll <scripthook.dll>] [--cwd <dir>] [--wait] [-- <arguments>]
  shk-loader --pid <process id> [--dll <scripthook.dll>]
  shk-loader --name <process.exe> [--dll <scripthook.dll>]

  --dll   default: scripthook.dll next to shk-loader.exe
  --wait  wait for the program to exit and return its exit code

The program can be the game's runtime (for example a runtime that runs the game's executable);
scripthook.dll finds the game's code in that process by itself. Single-player / offline only.";

fn main() {
    std::process::exit(match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("shk-loader: {e}");
            1
        }
    });
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn quote(a: &str) -> String {
    if a.is_empty() || a.contains([' ', '\t', '"']) {
        format!("\"{}\"", a.replace('"', "\\\""))
    } else {
        a.to_string()
    }
}

fn run() -> Result<i32, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let (args, rest) = match all.iter().position(|a| a == "--") {
        Some(i) => (all[..i].to_vec(), all[i + 1..].to_vec()),
        None => (all.clone(), Vec::new()),
    };
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    let dll = match arg(&args, "--dll") {
        Some(d) => PathBuf::from(d),
        None => std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("scripthook.dll"),
    };
    let dll = std::fs::canonicalize(&dll).map_err(|e| format!("{}: {e}", dll.display()))?;
    let dll_s = dll.display().to_string().trim_start_matches(r"\\?\").to_string();
    if let Some(exe) = arg(&args, "--launch") {
        let exe_p = std::fs::canonicalize(&exe).map_err(|e| format!("{exe}: {e}"))?;
        let exe_s = exe_p.display().to_string().trim_start_matches(r"\\?\").to_string();
        let cwd = arg(&args, "--cwd").map(PathBuf::from).unwrap_or_else(|| exe_p.parent().unwrap().to_path_buf());
        let mut cmd: Vec<u16> = wide(&std::iter::once(quote(&exe_s)).chain(rest.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" "));
        // SAFETY: documented process creation with zeroed structures.
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let cw = wide(&cwd.display().to_string().trim_start_matches(r"\\?\").to_string());
            if CreateProcessW(std::ptr::null(), cmd.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), 1, 0, std::ptr::null_mut(), cw.as_ptr(), &mut si, &mut pi) == 0 {
                return Err(format!("could not start {exe_s} (error {})", GetLastError()));
            }
            println!("shk-loader: started {exe_s} (pid {})", pi.dwProcessId);
            let r = wait_ready(pi.hProcess).and_then(|_| inject(pi.hProcess, &dll_s));
            if let Err(e) = r {
                CloseHandle(pi.hThread);
                CloseHandle(pi.hProcess);
                return Err(e);
            }
            println!("shk-loader: loaded {dll_s}");
            let mut code = 0u32;
            if args.iter().any(|a| a == "--wait") {
                WaitForSingleObject(pi.hProcess, INFINITE);
                GetExitCodeProcess(pi.hProcess, &mut code);
            }
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Ok(code as i32);
        }
    }
    let pid = match (arg(&args, "--pid"), arg(&args, "--name")) {
        (Some(p), _) => p.parse::<u32>().map_err(|_| format!("bad pid {p}"))?,
        (None, Some(n)) => find_process(&n).ok_or_else(|| format!("no running process named {n}"))?,
        _ => return Err(format!("nothing to do\n\n{USAGE}")),
    };
    // SAFETY: opening a process of the current user.
    unsafe {
        let h = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
        if h.is_null() {
            return Err(format!("cannot open process {pid} (error {})", GetLastError()));
        }
        let r = inject(h, &dll_s);
        CloseHandle(h);
        r?;
    }
    println!("shk-loader: loaded {dll_s} into process {pid}");
    Ok(0)
}

fn find_process(name: &str) -> Option<u32> {
    // SAFETY: toolhelp snapshot.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut pe);
        let mut found = None;
        while ok != 0 {
            if from_wide(&pe.szExeFile).eq_ignore_ascii_case(name) {
                found = Some(pe.th32ProcessID);
                break;
            }
            ok = Process32NextW(snap, &mut pe);
        }
        CloseHandle(snap);
        found
    }
}

/// Waits until the new process has kernel32 loaded (LoadLibraryW must exist there).
unsafe fn wait_ready(p: HANDLE) -> Result<(), String> {
    for _ in 0..500 {
        let mut mods = [std::ptr::null_mut::<c_void>(); 16];
        let mut need = 0u32;
        if K32EnumProcessModules(p, mods.as_mut_ptr(), (mods.len() * 8) as u32, &mut need) != 0 && need as usize / 8 >= 3 {
            return Ok(());
        }
        let mut code = 0u32;
        if GetExitCodeProcess(p, &mut code) != 0 && code != 259 {
            return Err(format!("the program exited with code {code} before the hook could load"));
        }
        Sleep(2);
    }
    Err("the program did not finish starting".into())
}

/// `LoadLibraryW(dll)` on a thread of the target process.
unsafe fn inject(p: HANDLE, dll: &str) -> Result<(), String> {
    let w = wide(dll);
    let bytes = w.len() * 2;
    let mem = VirtualAllocEx(p, std::ptr::null_mut(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if mem.is_null() {
        return Err(format!("VirtualAllocEx failed (error {})", GetLastError()));
    }
    let mut n = 0usize;
    if WriteProcessMemory(p, mem, w.as_ptr() as *const c_void, bytes, &mut n) == 0 {
        VirtualFreeEx(p, mem, 0, MEM_RELEASE);
        return Err(format!("WriteProcessMemory failed (error {})", GetLastError()));
    }
    // kernel32 is mapped at the same address in every process of this session
    let k32 = GetModuleHandleW(wide("kernel32.dll").as_ptr());
    let lla = GetProcAddress(k32, c"LoadLibraryW".as_ptr() as *const u8);
    let t = CreateRemoteThread(p, std::ptr::null_mut(), 0, lla, mem, 0, std::ptr::null_mut());
    if t.is_null() {
        VirtualFreeEx(p, mem, 0, MEM_RELEASE);
        return Err(format!("CreateRemoteThread failed (error {})", GetLastError()));
    }
    WaitForSingleObject(t, 30_000);
    let mut code = 0u32;
    GetExitCodeThread(t, &mut code);
    CloseHandle(t);
    VirtualFreeEx(p, mem, 0, MEM_RELEASE);
    if code == 0 {
        return Err(format!("LoadLibraryW({dll}) failed in the target process"));
    }
    Ok(())
}
