use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "poke", about = "Scheduled iMessage nudges through imsg")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create config, state, and log directories and write a starter config.
    Init,
    /// Run one scheduled launchd tick.
    Tick,
    /// Show resolved paths and current state.
    Show,
    /// Force-regenerate today's schedule.
    Regen,
    /// Install the per-user LaunchAgent plist.
    InstallAgent,
    /// Remove the LaunchAgent plist and print the unload command.
    UninstallAgent,
    /// Print the rendered LaunchAgent plist.
    PrintPlist,
}
