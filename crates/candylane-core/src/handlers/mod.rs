//! The three concrete `Handler` implementations (Lanes B/C/D).
//!
//! - [`winget::WingetHandler`] — winget package install/uninstall (Lane B, Windows-only,
//!   currently a `todo!()` stub; never invoked on the Linux vertical slice).
//! - [`dotfile::DotfileHandler`] — copy-with-verified-backup dotfile management (Lane C).
//! - [`script::ScriptHandler`] — post-install shell scripts with timeout group-kill (Lane D).

pub mod dotfile;
pub mod script;
pub mod winget;

pub use dotfile::DotfileHandler;
pub use script::ScriptHandler;
pub use winget::WingetHandler;
