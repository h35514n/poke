use anyhow::{Context, bail};
use std::fs;
use std::path::{Path, PathBuf};

const TEMPLATE: &str = include_str!("../assets/com.example.poke.plist.in");

pub fn render_plist(binary_path: &Path, log_dir: &Path) -> anyhow::Result<String> {
    if !binary_path.is_absolute() {
        bail!("binary path must be absolute: {}", binary_path.display());
    }
    if !log_dir.is_absolute() {
        bail!("log directory must be absolute: {}", log_dir.display());
    }
    let bundle_id = default_bundle_id()?;
    Ok(TEMPLATE
        .replace("{{BUNDLE_ID}}", &xml_escape(&bundle_id))
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
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", default_bundle_id()?)))
}

pub fn default_bundle_id() -> anyhow::Result<String> {
    let user = std::env::var("USER").context("USER must be set to render the LaunchAgent label")?;
    let user = user.trim();
    if user.is_empty() {
        bail!("USER must not be empty to render the LaunchAgent label");
    }
    Ok(format!("com.{user}.poke"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
    fn default_bundle_id_uses_current_user() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::set_var("USER", "alice");
        }
        assert_eq!(default_bundle_id().unwrap(), "com.alice.poke");
    }

    #[test]
    fn rendered_plist_uses_current_user_label() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::set_var("USER", "alice");
        }
        let plist =
            render_plist(Path::new("/usr/local/bin/poke"), Path::new("/tmp/poke/log")).unwrap();
        assert!(plist.contains("<string>com.alice.poke</string>"));
        assert!(!plist.contains("com.example.poke"));
    }
}
