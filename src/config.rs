use anyhow::{Context, bail};
use chrono::{NaiveTime, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_MESSAGE_CATEGORY: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub delivery: DeliveryConfig,
    pub schedule: ScheduleConfig,
    pub random: RandomConfig,
    #[serde(default, skip_serializing_if = "ScheduledConfig::is_empty")]
    pub scheduled: ScheduledConfig,
    #[serde(default, skip_serializing_if = "IntervalsConfig::is_empty")]
    pub intervals: IntervalsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryConfig {
    pub destination: String,
    pub imsg_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfig {
    pub start_hour: u32,
    pub end_hour: u32,
    pub random_per_day: RandomPerDay,
    pub random_min_spacing_minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RandomPerDay {
    pub default: usize,
    #[serde(default)]
    pub jitter: usize,
    #[serde(default, skip_serializing_if = "WeekdayOverrides::is_empty")]
    pub weekday: WeekdayOverrides,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WeekdayOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monday: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuesday: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wednesday: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thursday: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friday: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturday: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sunday: Option<usize>,
}

impl WeekdayOverrides {
    pub fn is_empty(&self) -> bool {
        self.monday.is_none()
            && self.tuesday.is_none()
            && self.wednesday.is_none()
            && self.thursday.is_none()
            && self.friday.is_none()
            && self.saturday.is_none()
            && self.sunday.is_none()
    }

    pub fn get(&self, weekday: Weekday) -> Option<usize> {
        match weekday {
            Weekday::Mon => self.monday,
            Weekday::Tue => self.tuesday,
            Weekday::Wed => self.wednesday,
            Weekday::Thu => self.thursday,
            Weekday::Fri => self.friday,
            Weekday::Sat => self.saturday,
            Weekday::Sun => self.sunday,
        }
    }

    pub fn values(&self) -> impl Iterator<Item = usize> + '_ {
        [
            self.monday,
            self.tuesday,
            self.wednesday,
            self.thursday,
            self.friday,
            self.saturday,
            self.sunday,
        ]
        .into_iter()
        .flatten()
    }
}

impl RandomPerDay {
    pub fn baseline_for(&self, weekday: Weekday) -> usize {
        self.weekday.get(weekday).unwrap_or(self.default)
    }

    pub fn max_possible(&self) -> usize {
        let max_baseline = self
            .weekday
            .values()
            .chain(std::iter::once(self.default))
            .max()
            .unwrap_or(self.default);
        max_baseline.saturating_add(self.jitter)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RandomConfig {
    pub items: Vec<MessageTemplate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ScheduledMessage {
    #[serde(with = "scheduled_time_format")]
    pub time: NaiveTime,
    pub text: String,
    #[serde(default = "default_message_category")]
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
        if self.schedule.random_per_day.default == 0 {
            bail!("schedule.random_per_day.default must be greater than 0");
        }
        if self
            .schedule
            .random_per_day
            .weekday
            .values()
            .any(|v| v == 0)
        {
            bail!("schedule.random_per_day weekday overrides must be greater than 0");
        }
        if self.schedule.random_min_spacing_minutes < 0 {
            bail!("schedule.random_min_spacing_minutes must not be negative");
        }
        if self.random.items.is_empty() {
            bail!("random.items must contain at least one message");
        }
        if self
            .random
            .items
            .iter()
            .any(|item| item.text.trim().is_empty())
        {
            bail!("random.items must not contain empty messages");
        }
        if self
            .random
            .items
            .iter()
            .any(|item| item.category.trim().is_empty())
        {
            bail!("random.items categories must not be empty");
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
    let worst_case = schedule.random_per_day.max_possible();
    let gap_count: i64 = worst_case.saturating_sub(1).try_into().unwrap_or(i64::MAX);
    let required_gap_minutes = gap_count.saturating_mul(schedule.random_min_spacing_minutes);
    if schedule.random_min_spacing_minutes > 0 && required_gap_minutes >= window_minutes {
        bail!(
            "schedule window is too small for {} pokes with {} minutes minimum spacing",
            worst_case,
            schedule.random_min_spacing_minutes
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
            random_per_day: RandomPerDay {
                default: 6,
                jitter: 0,
                weekday: WeekdayOverrides::default(),
            },
            random_min_spacing_minutes: 45,
        },
        random: RandomConfig {
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

    fn base_toml(random: &str) -> String {
        format!(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6

[random]
items = {random}
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
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6

[random]
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
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6

[random]
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
    fn string_only_random_items_default_to_default_category() {
        let config: Config =
            toml::from_str(&base_toml(r#"["Drink water.", "Stand up and stretch."]"#)).unwrap();
        assert_eq!(
            config.random.items,
            vec![
                MessageTemplate::new("Drink water.", DEFAULT_MESSAGE_CATEGORY),
                MessageTemplate::new("Stand up and stretch.", DEFAULT_MESSAGE_CATEGORY),
            ]
        );
    }

    #[test]
    fn mixed_random_item_shapes_parse_and_normalize() {
        let config: Config = toml::from_str(&base_toml(
            r#"[
  "Drink water.",
  { text = "Stand up and stretch.", category = "movement" },
  { text = "Review notes." }
]"#,
        ))
        .unwrap();
        assert_eq!(
            config.random.items,
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
        config.schedule.random_per_day.default = 3;
        config.schedule.random_min_spacing_minutes = 45;
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("too small"));
    }

    #[test]
    fn empty_random_category_is_rejected() {
        let mut config = default_config();
        config.random.items = vec![MessageTemplate::new("Drink water.", "")];
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("categories must not be empty"));
    }

    #[test]
    fn old_messages_section_is_rejected() {
        let err = toml::from_str::<Config>(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6

[messages]
items = ["Drink water."]
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field `messages`"));
    }

    #[test]
    fn old_schedule_count_and_spacing_are_rejected() {
        let err = toml::from_str::<Config>(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
pokes_per_day = 6
min_spacing_minutes = 45

[random]
items = ["Drink water."]
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field `pokes_per_day`"));
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

    #[test]
    fn random_per_day_table_parses() {
        let config: Config = toml::from_str(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6
jitter = 2

[schedule.random_per_day.weekday]
saturday = 3
sunday = 3

[random]
items = ["Drink water."]
"#,
        )
        .unwrap();
        assert_eq!(config.schedule.random_per_day.default, 6);
        assert_eq!(config.schedule.random_per_day.jitter, 2);
        assert_eq!(config.schedule.random_per_day.weekday.saturday, Some(3));
        assert_eq!(config.schedule.random_per_day.weekday.sunday, Some(3));
        assert_eq!(config.schedule.random_per_day.weekday.monday, None);
    }

    #[test]
    fn random_per_day_minimum_form_parses() {
        let config: Config = toml::from_str(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6

[random]
items = ["Drink water."]
"#,
        )
        .unwrap();
        assert_eq!(config.schedule.random_per_day.default, 6);
        assert_eq!(config.schedule.random_per_day.jitter, 0);
        assert!(config.schedule.random_per_day.weekday.is_empty());
    }

    #[test]
    fn legacy_scalar_random_per_day_is_rejected() {
        let err = toml::from_str::<Config>(
            r#"
[delivery]
destination = "+15555555555"
imsg_path = "/tmp/imsg"

[schedule]
start_hour = 9
end_hour = 21
random_per_day = 6
random_min_spacing_minutes = 45

[random]
items = ["Drink water."]
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("random_per_day"), "error was: {err}");
    }

    #[test]
    fn zero_random_per_day_default_is_rejected() {
        let mut config = default_config();
        config.schedule.random_per_day.default = 0;
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("random_per_day.default must be greater than 0"));
    }

    #[test]
    fn zero_weekday_override_is_rejected() {
        let mut config = default_config();
        config.schedule.random_per_day.weekday.saturday = Some(0);
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("weekday overrides must be greater than 0"));
    }

    #[test]
    fn density_validation_uses_max_possible_count() {
        let mut config = default_config();
        // window: 9..21 = 720 minutes
        // with default=6 + jitter=20 + spacing=45 -> 25 pokes, 24 gaps * 45 = 1080 >= 720
        config.schedule.random_per_day.default = 6;
        config.schedule.random_per_day.jitter = 20;
        config.schedule.random_min_spacing_minutes = 45;
        let err = config.validate(false).unwrap_err().to_string();
        assert!(err.contains("too small"), "error was: {err}");
    }
}
