use anyhow::{Context, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub const PLIST_NAME: &str = "com.example.poke.plist";
const TEMPLATE: &str = include_str!("../assets/com.example.poke.plist.in");

pub fn render_plist(binary_path: &Path, log_dir: &Path) -> anyhow::Result<String> {
    if !binary_path.is_absolute() {
        bail!("binary path must be absolute: {}", binary_path.display());
    }
    if !log_dir.is_absolute() {
        bail!("log directory must be absolute: {}", log_dir.display());
    }
    Ok(TEMPLATE
        .replace(
            "{{BINARY_PATH}}",
            &xml_escape(&binary_path.display().to_string()),
        )
        .replace(
            "{{STATE_LOG_DIR}}",
            &xml_escape(&log_dir.display().to_string()),
        ))
}

pub fn install_plist(contents: &str) -> anyhow::Result<PathBuf> {
    let path = plist_path()?;
    let parent = path
        .parent()
        .with_context(|| format!("plist path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(&path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

pub fn uninstall_plist() -> anyhow::Result<PathBuf> {
    let path = plist_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(path)
}

pub fn plist_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .context("HOME must be set to an absolute path")?;
    Ok(home.join("Library").join("LaunchAgents").join(PLIST_NAME))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
