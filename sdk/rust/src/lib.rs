//! # scripthook-sdk
//!
//! The Rust SDK of gta-vi-scripthook: write plugins for its hook (scripthook.dll), which loads
//! them from its `plugins` folder and serves them `scripthook.h`. Standard library only.
//!
//! ```ignore
//! use scripthook_sdk::{self as shk, natives::invoke};
//!
//! struct Hello;
//!
//! impl shk::Plugin for Hello {
//!     fn load(host: shk::Host) -> Result<Self, String> {
//!         host.info("hello from Rust");
//!         host.hotkey(shk::keys::vk_from_name("F7").unwrap(), 0, || shk::host().notify("F7", 2.0));
//!         host.start_script("main", || loop {
//!             let ped: i32 = invoke("PLAYER::PLAYER_PED_ID", ()).unwrap_or(0);
//!             shk::wait(1000);
//!         });
//!         Ok(Hello)
//!     }
//! }
//!
//! shk::export_plugin!(Hello, name = "hello", version = "0.1.0", author = "you", description = "says hello");
//! ```
//!
//! Single-player / offline only: nothing in the SDK touches online features.

pub mod config;
pub mod draw;
pub mod ffi;
pub mod font;
pub mod hash;
pub mod host;
pub mod keys;
pub mod menu;
pub mod natives;
pub mod png;
#[doc(hidden)]
pub mod rt;
#[doc(hidden)]
pub mod services;
#[doc(hidden)]
pub mod srt2;
pub mod win;

pub use hash::joaat;
pub use host::{wait, Host, Module, ScriptMode, ScriptStatus};

pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The host. Panics when called before the plugin is loaded.
pub fn host() -> Host {
    let p = rt::host_ptr();
    assert!(!p.is_null(), "scripthook-sdk: the host is not set (plugin not loaded)");
    // SAFETY: set at load, valid until unload.
    unsafe { Host::from_raw(p) }
}

/// True once the plugin is loaded.
pub fn loaded() -> bool {
    !rt::host_ptr().is_null()
}

/// A plugin. `load` runs on the host's main thread; keep what you need in `Self`.
pub trait Plugin: Sized + Send + 'static {
    fn load(host: Host) -> Result<Self, String>;
    /// Before the plugin is unloaded (scripts and callbacks are removed by the host afterwards).
    fn unload(&mut self) {}
}

/// Exports the plugin's entry points: `shk_plugin_load` / `shk_plugin_unload` /
/// `shk_plugin_info` (scripthook.h) and `srt_plugin_init` / `srt_plugin_shutdown` (the adapter
/// for Sonoma Runtime's plugin API version 2).
#[macro_export]
macro_rules! export_plugin {
    ($ty:ty, name = $name:expr, version = $ver:expr, author = $author:expr, description = $desc:expr) => {
        static __SHK_INSTANCE: ::std::sync::Mutex<::std::option::Option<$ty>> = ::std::sync::Mutex::new(::std::option::Option::None);

        #[no_mangle]
        pub unsafe extern "C" fn shk_plugin_load(host: *const $crate::ffi::ShkHost, me: *mut $crate::ffi::ShkPlugin) -> i32 {
            $crate::rt::load::<$ty>(host, me, &__SHK_INSTANCE)
        }

        #[no_mangle]
        pub unsafe extern "C" fn shk_plugin_unload() {
            $crate::rt::unload::<$ty>(&__SHK_INSTANCE)
        }

        #[no_mangle]
        pub extern "C" fn shk_plugin_info() -> *const $crate::ffi::ShkPluginInfo {
            static INFO: $crate::ffi::StaticPluginInfo = $crate::ffi::StaticPluginInfo($crate::ffi::ShkPluginInfo {
                size: ::std::mem::size_of::<$crate::ffi::ShkPluginInfo>() as u32,
                abi_version: $crate::ffi::SHK_ABI_VERSION,
                name: concat!($name, "\0").as_ptr() as *const ::std::ffi::c_char,
                version: concat!($ver, "\0").as_ptr() as *const ::std::ffi::c_char,
                author: concat!($author, "\0").as_ptr() as *const ::std::ffi::c_char,
                description: concat!($desc, "\0").as_ptr() as *const ::std::ffi::c_char,
            });
            &INFO.0
        }

        #[no_mangle]
        pub unsafe extern "C" fn srt_plugin_init(api: *const ::std::ffi::c_void, me: *mut $crate::ffi::ShkPlugin) -> i32 {
            $crate::srt2::init(api, me, shk_plugin_load)
        }

        #[no_mangle]
        pub unsafe extern "C" fn srt_plugin_shutdown() {
            $crate::srt2::shutdown(shk_plugin_unload)
        }
    };
}

/// `info!("...", args)` through the host log.
#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::host().info(&format!($($t)*)) };
}

/// `warn!("...", args)` through the host log.
#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => { $crate::host().warn(&format!($($t)*)) };
}
