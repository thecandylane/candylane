//! candylane — the CLI surface (Lane F).
//!
//! SCAFFOLD: command wiring is sketched; each arm calls into candylane-core/-crypto.
//! `--resume` is intentionally absent (cut from Phase 1, Decision #5).

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "candylane",
    version,
    about = "Build your perfect machine once. Pull it anywhere."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate your identity and set up ~/.candylane/.
    Init,
    /// Show exactly what a pull would change — without changing anything.
    Diff { profile: String },
    /// Apply a profile to this machine.
    Pull { profile: String },
    /// Roll back the last operation.
    Revert,
    /// Recover from an interrupted pull (reconcile the in-flight action, then roll back to clean).
    Recover,
    /// Show everything Candylane has done on this machine.
    History,
    /// Validate machine state against the state DB.
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => {
            let id = candylane_crypto::Identity::generate()?;
            println!("identity created: {:?}", id.verifying_key());
            Ok(())
        }
        Command::Diff { profile } => {
            let _ = profile;
            todo!("load profile → engine.diff() → print plan with best_effort/one_way flags")
        }
        Command::Pull { profile } => {
            let _ = profile;
            todo!("preflight (winget? reboot-pending?) → engine.pull()")
        }
        Command::Revert => todo!("engine.revert(last_op)"),
        Command::Recover => todo!("engine.recover() — reconcile in-flight, then rollback"),
        Command::History => todo!("store.list_operations() → print"),
        Command::Status => todo!("validate machine state vs state.db"),
    }
}
