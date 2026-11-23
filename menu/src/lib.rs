//! The gta-vi-scripthook mod menu.
//!
//! * [`model`]: pages and items registered by plugins through the menu service.
//! * [`nav`]: selection, scrolling, number / text entry, filters.
//! * [`render`]: the look (header bar, breadcrumb, rows, switches, sliders, footer hints,
//!   description panel, toasts, HUD lines) on any [`scripthook_sdk::draw::Painter`].
//! * [`core`]: model + navigation + the menu's own pages.
//! * [`input`]: keyboard / gamepad mapping with auto-repeat.
//! * [`service`]: the C ABI service "scripthook.menu" with its two backends (drawn, mirror).
//! * [`plugin`]: the plugin entry points.

pub mod core;
pub mod input;
pub mod model;
pub mod nav;
pub mod plugin;
pub mod render;
pub mod service;
