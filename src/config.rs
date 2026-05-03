use anyhow::{Context, bail};
use chrono::NaiveTime;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_MESSAGE_CATEGORY: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub delivery: DeliveryConfig,
    pub schedule: ScheduleConfig,
    pub messages: MessagesConfig,
    #[serde(default, skip_serializing_if = "ScheduledConfig::is_empty")]
    pub scheduled: ScheduledConfig,
    #[serde(default, skip_serializing_if = "IntervalsConfig::is_empty")]
    pub intervals: IntervalsConfig,
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
    pub items: Vec<MessageTemplate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledConfig {
    #[serde(default)]
    pub items: Vec<ScheduledMessage>,
}

impl ScheduledConfig {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntervalsConfig {
    #[serde(default)]
    pub items: Vec<IntervalMessage>,
}

impl IntervalsConfig {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageTemplate {
    pub text: String,
    pub category: String,
}

impl MessageTemplate {
    pub fn new(text: impl Into<String>, category: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            category: category.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledMessage {
    #[serde(with = "scheduled_time_format")]
    pub time: NaiveTime,
    pub text: String,
    #[serde(default = "default_message_category")]
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntervalMessage {
    pub every_minutes: u32,
    pub text: String,
    #[serde(default = "default_message_category")]
    pub category: String,
}

#[cfg(test)]
impl IntervalMessage {
    pub fn new(every_minutes: u32, text: impl Into<String>, category: impl Into<String>) -> Self {
        Self {
            every_minutes,
            text: text.into(),
            category: category.into(),
        }
    }
}

#[cfg(test)]
impl ScheduledMessage {
    pub fn new(time: NaiveTime, text: impl Into<String>, category: impl Into<String>) -> Self {
        Self {
            time,
            text: text.into(),
            category: category.into(),
        }
    }
}

impl<'de> Deserialize<'de> for MessageTemplate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawMessageTemplate {
            Text(String),
            Structured {
                text: String,
                #[serde(default)]
                category: Option<String>,
            },
        }

        let raw = RawMessageTemplate::deserialize(deserializer)?;
        Ok(match raw {
            RawMessageTemplate::Text(text) => MessageTemplate::new(text, DEFAULT_MESSAGE_CATEGORY),
            RawMessageTemplate::Structured { text, category } => MessageTemplate::new(
                text,
                category.unwrap_or_else(|| DEFAULT_MESSAGE_CATEGORY.to_string()),
            ),
        })
    }
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
            .any(|item| item.text.trim().is_empty())
        {
            bail!("messages.items must not contain empty messages");
        }
        if self
            .messages
            .items
            .iter()
            .any(|item| item.category.trim().is_empty())
        {
            bail!("messages.items categories must not be empty");
        }
        if self
            .scheduled
            .items
            .iter()
            .any(|item| item.text.trim().is_empty())
        {
            bail!("scheduled.items must not contain empty messages");
        }
        if self
            .scheduled
            .items
            .iter()
            .any(|item| item.category.trim().is_empty())
        {
            bail!("scheduled.items categories must not be empty");
        }
        if self
            .intervals
            .items
            .iter()
            .any(|item| item.every_minutes == 0)
        {
            bail!("intervals.items every_minutes must be greater than 0");
        }
        if self
            .intervals
            .items
            .iter()
            .any(|item| item.every_minutes < 5)
        {
            bail!("intervals.items every_minutes must be at least 5");
        }
        if self
            .intervals
            .items
            .iter()
            .any(|item| item.text.trim().is_empty())
        {
            bail!("intervals.items must not contain empty messages");
        }
        if self
            .intervals
            .items
            .iter()
            .any(|item| item.category.trim().is_empty())
        {
            bail!("intervals.items categories must not be empty");
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
                MessageTemplate::new("Update openclaw context.", "focus"),
                MessageTemplate::new("Drink water.", "hydration"),
                MessageTemplate::new("Stand up and stretch.", "mobility"),
                MessageTemplate::new("Walk around for two minutes.", "movement"),
                MessageTemplate::new("Do ten air squats.", "movement"),
            ],
        },
        scheduled: ScheduledConfig::default(),
        intervals: IntervalsConfig::default(),
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

fn default_message_category() -> String {
    DEFAULT_MESSAGE_CATEGORY.to_string()
}

fn parse_scheduled_time(input: &str) -> Option<NaiveTime> {
    let trimmed = input.trim();
    if let Ok(time) = NaiveTime::parse_from_str(trimmed, "%H:%M") {
        return Some(time);
    }
    let compact = trimmed.to_ascii_lowercase().replace(' ', "");
    for format in ["%I:%M%P", "%I%P"] {
        if let Ok(time) = NaiveTime::parse_from_str(&compact, format) {
            return Some(time);
        }
    }
    None
}

mod scheduled_time_format {
    use super::*;
    use serde::de::Error;

    pub fn serialize<S>(time: &NaiveTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&time.format("%H:%M").to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        parse_scheduled_time(&raw)
            .ok_or_else(|| D::Error::custom("scheduled time must be HH:MM or h:MMam/pm"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_toml(messages: &str) -> String {
        format!(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
pokes_per_day = 6
min_spacing_minutes = 45

[messages]
items = {messages}
"#
        )
    }

    fn base_toml_with_scheduled(scheduled: &str) -> String {
        format!(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
pokes_per_day = 6
min_spacing_minutes = 45

[messages]
items = ["Drink water."]

[scheduled]
items = {scheduled}
"#
        )
    }

    fn base_toml_with_intervals(intervals: &str) -> String {
        format!(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
pokes_per_day = 6
min_spacing_minutes = 45

[messages]
items = ["Drink water."]

[intervals]
items = {intervals}
"#
        )
    }

    #[test]
    fn default_config_is_valid_without_existing_imsg_for_init() {
        default_config().validate(false).unwrap();
    }

    #[test]
    fn string_only_messages_default_to_default_category() {
        let config: Config =
            toml::from_str(&base_toml(r#"["Drink water.", "Stand up and stretch."]"#)).unwrap();
        assert_eq!(
            config.messages.items,
            vec![
                MessageTemplate::new("Drink water.", DEFAULT_MESSAGE_CATEGORY),
                MessageTemplate::new("Stand up and stretch.", DEFAULT_MESSAGE_CATEGORY),
            ]
        );
    }

    #[test]
    fn mixed_message_shapes_parse_and_normalize() {
        let config: Config = toml::from_str(&base_toml(
            r#"[
  "Drink water.",
  { text = "Stand up and stretch.", category = "movement" },
  { text = "Review notes." }
]"#,
        ))
        .unwrap();
        assert_eq!(
            config.messages.items,
            vec![
                MessageTemplate::new("Drink water.", DEFAULT_MESSAGE_CATEGORY),
                MessageTemplate::new("Stand up and stretch.", "movement"),
                MessageTemplate::new("Review notes.", DEFAULT_MESSAGE_CATEGORY),
            ]
        );
    }

    #[test]
    fn scheduled_messages_parse_and_default_category() {
        let config: Config = toml::from_str(&base_toml_with_scheduled(
            r#"[
  { time = "15:00", text = "Afternoon check-in." },
  { time = "3:30pm", text = "Later check-in.", category = "fixed" }
]"#,
        ))
        .unwrap();
        assert_eq!(
            config.scheduled.items,
            vec![
                ScheduledMessage::new(
                    NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
                    "Afternoon check-in.",
                    DEFAULT_MESSAGE_CATEGORY
                ),
                ScheduledMessage::new(
                    NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
                    "Later check-in.",
                    "fixed"
                ),
            ]
        );
    }

    #[test]
    fn invalid_scheduled_time_is_rejected() {
        let err = toml::from_str::<Config>(&base_toml_with_scheduled(
            r#"[{ time = "noonish", text = "Afternoon check-in." }]"#,
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("scheduled time"));
    }

    #[test]
    fn interval_messages_parse_and_default_category() {
        let config: Config = toml::from_str(&base_toml_with_intervals(
            r#"[
  { every_minutes = 60, text = "Drink water." },
  { every_minutes = 90, text = "Stretch.", category = "mobility" }
]"#,
        ))
        .unwrap();
        assert_eq!(
            config.intervals.items,
            vec![
                IntervalMessage::new(60, "Drink water.", DEFAULT_MESSAGE_CATEGORY),
                IntervalMessage::new(90, "Stretch.", "mobility"),
            ]
        );
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

    #[test]
    fn empty_message_category_is_rejected() {
        let mut config = default_config();
        config.messages.items = vec![MessageTemplate::new("Drink water.", "")];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("categories must not be empty"));
    }

    #[test]
    fn empty_scheduled_message_is_rejected() {
        let mut config = default_config();
        config.scheduled.items = vec![ScheduledMessage::new(
            NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
            "",
            "fixed",
        )];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("scheduled.items must not contain empty"));
    }

    #[test]
    fn empty_scheduled_category_is_rejected() {
        let mut config = default_config();
        config.scheduled.items = vec![ScheduledMessage::new(
            NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
            "Afternoon check-in.",
            "",
        )];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("scheduled.items categories must not be empty"));
    }

    #[test]
    fn zero_interval_minutes_is_rejected() {
        let mut config = default_config();
        config.intervals.items = vec![IntervalMessage::new(0, "Drink water.", "hydration")];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("every_minutes must be greater than 0"));
    }

    #[test]
    fn too_small_interval_minutes_is_rejected() {
        let mut config = default_config();
        config.intervals.items = vec![IntervalMessage::new(4, "Drink water.", "hydration")];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("every_minutes must be at least 5"));
    }

    #[test]
    fn empty_interval_message_is_rejected() {
        let mut config = default_config();
        config.intervals.items = vec![IntervalMessage::new(60, "", "hydration")];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("intervals.items must not contain empty"));
    }

    #[test]
    fn empty_interval_category_is_rejected() {
        let mut config = default_config();
        config.intervals.items = vec![IntervalMessage::new(60, "Drink water.", "")];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("intervals.items categories must not be empty"));
    }
}
