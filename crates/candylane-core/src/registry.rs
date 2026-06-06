//! The concrete [`HandlerRegistry`] used in production: one of each handler, dispatched
//! by [`HandlerKind`].
//!
//! Built once at startup. The winget handler shells `winget.exe` (Windows); on a
//! non-Windows host its executor is a loud stub, so dispatching a winget item off-Windows
//! errors rather than silently misbehaving. The dotfile + script handlers are
//! cross-platform.

use crate::engine::HandlerRegistry;
use crate::handler::Handler;
use crate::handlers::{DotfileHandler, ScriptHandler, WingetHandler};
use crate::types::HandlerKind;

/// Owns the three concrete handlers and routes a [`HandlerKind`] to the right one.
pub struct Handlers {
    winget: WingetHandler,
    dotfile: DotfileHandler,
    script: ScriptHandler,
}

impl Handlers {
    pub fn new() -> Self {
        Handlers {
            winget: WingetHandler::new(),
            dotfile: DotfileHandler::new(),
            script: ScriptHandler::new(),
        }
    }
}

impl Default for Handlers {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlerRegistry for Handlers {
    fn get(&self, kind: HandlerKind) -> &dyn Handler {
        match kind {
            HandlerKind::Winget => &self.winget,
            HandlerKind::Dotfile => &self.dotfile,
            HandlerKind::Script => &self.script,
        }
    }
}
