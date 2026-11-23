//! The gta-vi-scripthook trainer: 44 data-driven features (TOML) running on natives through
//! the script hook, with memory cheats where natives are not known yet.
//!
//! * [`script`]: the feature statement language (parser, analysis, interpreter).
//! * [`features`]: feature definitions (`features/*.toml`).
//! * [`cheats`]: memory locations and cheats (`cheats.toml`).
//! * [`lists`]: the owner's trainer data lists.
//! * [`runtime`]: availability, the toggle script, runs, memory cheats, fly, HUD, saved state.
//! * [`ui`]: menu pages (menu service or the host's basic menu).
//! * [`auto`]: automation scripts.
//! * [`plugin`]: the plugin entry points.

pub mod auto;
pub mod cheats;
pub mod features;
pub mod lists;
pub mod plugin;
pub mod runtime;
pub mod script;
pub mod ui;
