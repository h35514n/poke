use anyhow::{Context, bail};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PokePaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub state_file: PathBuf,
    pub lock_file: PathBuf,
    pub log_dir: PathBuf,
}

impl PokePaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let home = home_dir()?;
        Ok(Self::from_env(&home))
    }

    pub fn from_env(home: &Path) -> Self {
        let config_base =
            absolute_env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let state_base = absolute_env_path("XDG_STATE_HOME")
            .unwrap_or_else(|| home.join(".local").join("state"));
        Self::from_bases(config_base, state_base)
    }

    pub fn from_bases(config_base: PathBuf, state_base: PathBuf) -> Self {
        let config_dir = config_base.join("poke");
        let state_dir = state_base.join("poke");
        let log_dir = state_dir.join("log");
        let config_file = config_dir.join("config.toml");
        let state_file = state_dir.join("state.json");
        let lock_file = state_dir.join("state.lock");
        Self {
            config_dir,
            config_file,
            state_dir,
            state_file,
            lock_file,
            log_dir,
        }
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        std::fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        std::fs::create_dir_all(&self.log_dir)
            .with_context(|| format!("failed to create {}", self.log_dir.display()))?;
        Ok(())
    }
}

fn absolute_env_path(name: &str) -> Option<PathBuf> {
    let value = env::var_os(name)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

fn home_dir() -> anyhow::Result<PathBuf> {
    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home);
        if path.is_absolute() {
            return Ok(path);
        }
        bail!("HOME must be an absolute path");
    }
    bail!("HOME is not set")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn xdg_paths_fall_back_when_unset_or_empty() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
            env::set_var("XDG_STATE_HOME", "");
        }
        let paths = PokePaths::from_env(Path::new("/Users/alice"));
        assert_eq!(
            paths.config_file,
            PathBuf::from("/Users/alice/.config/poke/config.toml")
        );
        assert_eq!(
            paths.state_file,
            PathBuf::from("/Users/alice/.local/state/poke/state.json")
        );
        assert_eq!(
            paths.log_dir,
            PathBuf::from("/Users/alice/.local/state/poke/log")
        );
    }

    #[test]
    fn relative_xdg_paths_are_ignored() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", "relative/config");
            env::set_var("XDG_STATE_HOME", "relative/state");
        }
        let paths = PokePaths::from_env(Path::new("/Users/alice"));
        assert_eq!(paths.config_dir, PathBuf::from("/Users/alice/.config/poke"));
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/Users/alice/.local/state/poke")
        );
    }
}
