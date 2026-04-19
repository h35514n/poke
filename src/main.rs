mod cli;
mod config;
mod delivery;
mod launchagent;
mod paths;
mod schedule;
mod state;
mod tick;
mod util;

use anyhow::Context;
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let paths = paths::PokePaths::resolve()?;

    match cli.command {
        cli::Command::Init => {
            paths.ensure_dirs()?;
            config::write_default_config_if_absent(&paths.config_file)?;
            println!("config: {}", paths.config_file.display());
            println!("state: {}", paths.state_file.display());
            println!("logs: {}", paths.log_dir.display());
        }
        cli::Command::Tick => tick::run_tick(&paths)?,
        cli::Command::Show => {
            let output = tick::show(&paths)?;
            print!("{output}");
        }
        cli::Command::Regen => {
            tick::regen_today(&paths)?;
            println!("regenerated today's schedule");
        }
        cli::Command::InstallAgent => {
            paths.ensure_dirs()?;
            let binary_path =
                std::env::current_exe().context("failed to resolve current binary path")?;
            let plist = launchagent::render_plist(&binary_path, &paths.log_dir)?;
            let path = launchagent::install_plist(&plist)?;
            println!("installed {}", path.display());
            println!(
                "load with: launchctl bootstrap gui/$(id -u) {}",
                path.display()
            );
        }
        cli::Command::UninstallAgent => {
            let path = launchagent::uninstall_plist()?;
            println!("removed {}", path.display());
            println!(
                "unload with: launchctl bootout gui/$(id -u) {}",
                path.display()
            );
        }
        cli::Command::PrintPlist => {
            let binary_path =
                std::env::current_exe().context("failed to resolve current binary path")?;
            let plist = launchagent::render_plist(&binary_path, &paths.log_dir)?;
            print!("{plist}");
        }
    }

    Ok(())
}
