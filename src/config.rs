use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub delivery: DeliveryConfig,
    pub schedule: ScheduleConfig,
    pub messages: MessagesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryConfig {
    pub destination: String,
    pub imsg_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleConfig {
    pub start_hour: u32,
    pub end_hour: u32,
    pub pokes_per_day: usize,
    pub min_spacing_minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessagesConfig {
    pub items: Vec<String>,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config {}", path.display()))?;
        config.validate(true)?;
        Ok(config)
    }

    pub fn validate(&self, require_imsg_exists: bool) -> anyhow::Result<()> {
        if self.delivery.destination.trim().is_empty() {
            bail!("delivery.destination must not be empty");
        }
        if !self.delivery.imsg_path.is_absolute() {
            bail!("delivery.imsg_path must be an absolute path");
        }
        if require_imsg_exists && !self.delivery.imsg_path.exists() {
            bail!(
                "delivery.imsg_path does not exist: {}",
                self.delivery.imsg_path.display()
            );
        }
        if self.schedule.start_hour >= 24 {
            bail!("schedule.start_hour must be in 0..=23");
        }
        if self.schedule.end_hour > 24 {
            bail!("schedule.end_hour must be in 1..=24");
        }
        if self.schedule.end_hour <= self.schedule.start_hour {
            bail!("schedule.end_hour must be greater than schedule.start_hour");
        }
        if self.schedule.pokes_per_day == 0 {
            bail!("schedule.pokes_per_day must be greater than 0");
        }
        if self.schedule.min_spacing_minutes < 0 {
            bail!("schedule.min_spacing_minutes must not be negative");
        }
        if self.messages.items.is_empty() {
            bail!("messages.items must contain at least one message");
        }
        if self
            .messages
            .items
            .iter()
            .any(|item| item.trim().is_empty())
        {
            bail!("messages.items must not contain empty messages");
        }
        validate_density(&self.schedule)?;
        Ok(())
    }
}

pub fn validate_density(schedule: &ScheduleConfig) -> anyhow::Result<()> {
    let window_minutes = ((schedule.end_hour - schedule.start_hour) as i64) * 60;
    let gap_count: i64 = schedule
        .pokes_per_day
        .saturating_sub(1)
        .try_into()
        .unwrap_or(i64::MAX);
    let required_gap_minutes = gap_count.saturating_mul(schedule.min_spacing_minutes);
    if schedule.min_spacing_minutes > 0 && required_gap_minutes >= window_minutes {
        bail!(
            "schedule window is too small for {} pokes with {} minutes minimum spacing",
            schedule.pokes_per_day,
            schedule.min_spacing_minutes
        );
    }
    Ok(())
}

pub fn default_config() -> Config {
    Config {
        delivery: DeliveryConfig {
            destination: "+15555555555".to_string(),
            imsg_path: PathBuf::from("/opt/homebrew/bin/imsg"),
        },
        schedule: ScheduleConfig {
            start_hour: 9,
            end_hour: 21,
            pokes_per_day: 6,
            min_spacing_minutes: 45,
        },
        messages: MessagesConfig {
            items: vec![
                "Update openclaw context.".to_string(),
                "Drink water.".to_string(),
                "Stand up and stretch.".to_string(),
                "Walk around for two minutes.".to_string(),
                "Do ten air squats.".to_string(),
            ],
        },
    }
}

pub fn default_config_toml() -> anyhow::Result<String> {
    toml::to_string_pretty(&default_config()).context("failed to serialize default config")
}

pub fn write_default_config_if_absent(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .with_context(|| format!("config path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(path, default_config_toml()?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid_without_existing_imsg_for_init() {
        default_config().validate(false).unwrap();
    }

    #[test]
    fn infeasible_spacing_is_rejected() {
        let mut config = default_config();
        config.schedule.start_hour = 9;
        config.schedule.end_hour = 10;
        config.schedule.pokes_per_day = 3;
        config.schedule.min_spacing_minutes = 45;
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("too small"));
    }
}
