use crate::config::{Config, MessageTemplate};
use crate::state::{PendingPoke, PokeKind, RecentMessage};
use anyhow::{Context, bail};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, NaiveDate, TimeZone, Timelike,
};
use rand::Rng;
use rand::seq::SliceRandom;
use std::collections::BTreeMap;

const MAX_ATTEMPTS: usize = 1_000;

pub fn generate_for_date<R: Rng + ?Sized>(
    config: &Config,
    recent_history: &[RecentMessage],
    date: NaiveDate,
    rng: &mut R,
) -> anyhow::Result<Vec<PendingPoke>> {
    let interval = active_interval(date, config.schedule.start_hour, config.schedule.end_hour)?;
    let mut pending = generate_in_interval(config, recent_history, date, interval, rng)?;
    pending.extend(generate_scheduled_for_date(config, date)?);
    pending.extend(generate_interval_for_date(config, date, interval)?);
    pending.sort_by_key(|poke| poke.at);
    Ok(pending)
}

fn generate_in_interval<R: Rng + ?Sized>(
    config: &Config,
    recent_history: &[RecentMessage],
    date: NaiveDate,
    interval: ActiveInterval,
    rng: &mut R,
) -> anyhow::Result<Vec<PendingPoke>> {
    let count = config.schedule.pokes_per_day;
    let total_seconds = (interval.end - interval.start).num_seconds();
    let min_spacing = Duration::minutes(config.schedule.min_spacing_minutes);
    let required = min_spacing
        .num_seconds()
        .saturating_mul(count.saturating_sub(1).try_into().unwrap_or(i64::MAX));
    if min_spacing > Duration::zero() && required >= total_seconds {
        bail!(
            "schedule density is infeasible: window is too small for {} pokes with {} minutes minimum spacing",
            count,
            config.schedule.min_spacing_minutes
        );
    }

    for _ in 0..MAX_ATTEMPTS {
        let mut times = Vec::with_capacity(count);
        for index in 0..count {
            let seg_start = total_seconds * index as i64 / count as i64;
            let seg_end = total_seconds * (index as i64 + 1) / count as i64;
            let offset = if seg_end > seg_start {
                rng.gen_range(seg_start..seg_end)
            } else {
                seg_start
            };
            times.push(interval.start + Duration::seconds(offset));
        }
        times.sort();
        if respects_min_spacing(&times, min_spacing) {
            let messages = select_messages(&config.messages.items, count, recent_history, rng);
            return Ok(times
                .into_iter()
                .enumerate()
                .map(|(index, at)| PendingPoke {
                    id: format!("{date}-random-{index}"),
                    at,
                    message: messages[index].text.clone(),
                    category: messages[index].category.clone(),
                    kind: PokeKind::Random,
                })
                .collect());
        }
    }

    bail!(
        "schedule density is infeasible after {MAX_ATTEMPTS} attempts: window is too small for {} pokes with {} minutes minimum spacing",
        count,
        config.schedule.min_spacing_minutes
    )
}

pub fn is_within_active_window(now: DateTime<FixedOffset>, start_hour: u32, end_hour: u32) -> bool {
    let hour = now.hour();
    hour >= start_hour && hour < end_hour
}

pub fn active_interval(
    date: NaiveDate,
    start_hour: u32,
    end_hour: u32,
) -> anyhow::Result<ActiveInterval> {
    let start = local_datetime(date, start_hour)?;
    let end = if end_hour == 24 {
        local_datetime(
            date.succ_opt()
                .with_context(|| format!("failed to advance date {date}"))?,
            0,
        )?
    } else {
        local_datetime(date, end_hour)?
    };
    if end <= start {
        bail!("active window end must be after start for {date}");
    }
    Ok(ActiveInterval { start, end })
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveInterval {
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
}

fn generate_scheduled_for_date(
    config: &Config,
    date: NaiveDate,
) -> anyhow::Result<Vec<PendingPoke>> {
    config
        .scheduled
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Ok(PendingPoke {
                id: format!("{date}-scheduled-{index}"),
                at: local_datetime_at_time(date, item.time)?,
                message: item.text.clone(),
                category: item.category.clone(),
                kind: PokeKind::Scheduled,
            })
        })
        .collect()
}

fn generate_interval_for_date(
    config: &Config,
    date: NaiveDate,
    interval: ActiveInterval,
) -> anyhow::Result<Vec<PendingPoke>> {
    let mut pending = Vec::new();
    for (item_index, item) in config.intervals.items.iter().enumerate() {
        let step = Duration::minutes(item.every_minutes.into());
        let mut at = interval.start;
        let mut slot_index = 0;
        while at < interval.end {
            pending.push(PendingPoke {
                id: format!("{date}-interval-{item_index}-{slot_index}"),
                at,
                message: item.text.clone(),
                category: item.category.clone(),
                kind: PokeKind::Interval,
            });
            at += step;
            slot_index += 1;
        }
    }
    Ok(pending)
}

fn local_datetime(date: NaiveDate, hour: u32) -> anyhow::Result<DateTime<FixedOffset>> {
    let time = chrono::NaiveTime::from_hms_opt(hour, 0, 0)
        .with_context(|| format!("invalid local hour {hour:02}:00"))?;
    local_datetime_at_time(date, time)
}

fn local_datetime_at_time(
    date: NaiveDate,
    time: chrono::NaiveTime,
) -> anyhow::Result<DateTime<FixedOffset>> {
    let local = match Local.with_ymd_and_hms(
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second(),
    ) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => bail!("local time {date} {} does not exist", time.format("%H:%M")),
    };
    Ok(local.fixed_offset())
}

fn respects_min_spacing(times: &[DateTime<FixedOffset>], min_spacing: Duration) -> bool {
    times
        .windows(2)
        .all(|pair| pair[1] - pair[0] >= min_spacing)
}

fn select_messages<R: Rng + ?Sized>(
    messages: &[MessageTemplate],
    count: usize,
    recent_history: &[RecentMessage],
    rng: &mut R,
) -> Vec<MessageTemplate> {
    let category_members = category_members(messages);
    let mut unseen = vec![true; messages.len()];
    let mut working_history = recent_history.to_vec();
    let mut selected = Vec::with_capacity(count);

    for slot in 0..count {
        let remaining_slots = count - slot;
        let remaining_unseen = unseen.iter().filter(|is_unseen| **is_unseen).count();
        let category = select_category(
            &category_members,
            &unseen,
            &working_history,
            remaining_slots,
            remaining_unseen,
            rng,
        );
        let message_index = select_message_index(
            messages,
            &category_members,
            category,
            &unseen,
            &working_history,
            rng,
        );
        unseen[message_index] = false;
        let message = messages[message_index].clone();
        working_history.push(RecentMessage::new(
            message.text.clone(),
            message.category.clone(),
        ));
        selected.push(message);
    }

    selected
}

fn category_members(messages: &[MessageTemplate]) -> BTreeMap<String, Vec<usize>> {
    let mut categories = BTreeMap::new();
    for (index, message) in messages.iter().enumerate() {
        categories
            .entry(message.category.clone())
            .or_insert_with(Vec::new)
            .push(index);
    }
    categories
}

fn select_category<'a, R: Rng + ?Sized>(
    category_members: &'a BTreeMap<String, Vec<usize>>,
    unseen: &[bool],
    history: &[RecentMessage],
    remaining_slots: usize,
    remaining_unseen: usize,
    rng: &mut R,
) -> &'a str {
    let mut candidates: Vec<&'a str> = category_members.keys().map(String::as_str).collect();

    if remaining_slots == remaining_unseen && remaining_unseen > 0 {
        candidates.retain(|category| category_has_unseen(category_members, category, unseen));
    }

    if let Some(last_category) = history.last().map(|entry| entry.category.as_str())
        && candidates.iter().any(|category| *category != last_category)
    {
        candidates.retain(|category| *category != last_category);
    }

    if candidates
        .iter()
        .any(|category| category_has_unseen(category_members, category, unseen))
    {
        candidates.retain(|category| category_has_unseen(category_members, category, unseen));
    }

    choose_least_recently_used(
        history,
        candidates,
        |entry, category| entry.category == *category,
        rng,
    )
}

fn select_message_index<R: Rng + ?Sized>(
    messages: &[MessageTemplate],
    category_members: &BTreeMap<String, Vec<usize>>,
    category: &str,
    unseen: &[bool],
    history: &[RecentMessage],
    rng: &mut R,
) -> usize {
    let mut candidates = category_members[category].clone();

    if candidates.iter().any(|index| unseen[*index]) {
        candidates.retain(|index| unseen[*index]);
    }

    if let Some(last) = history.last()
        && candidates
            .iter()
            .any(|index| !matches_recent_message(&messages[*index], last))
    {
        candidates.retain(|index| !matches_recent_message(&messages[*index], last));
    }

    *choose_least_recently_used(
        history,
        candidates.iter().collect(),
        |entry, index| matches_recent_message(&messages[**index], entry),
        rng,
    )
}

fn choose_least_recently_used<T, R, F>(
    history: &[RecentMessage],
    candidates: Vec<T>,
    matches: F,
    rng: &mut R,
) -> T
where
    T: Clone,
    R: Rng + ?Sized,
    F: Fn(&RecentMessage, &T) -> bool,
{
    let best_score = candidates
        .iter()
        .map(|candidate| recency_score(history, |entry| matches(entry, candidate)))
        .max()
        .unwrap_or(usize::MAX);
    let best: Vec<T> = candidates
        .into_iter()
        .filter(|candidate| recency_score(history, |entry| matches(entry, candidate)) == best_score)
        .collect();
    best.choose(rng).cloned().expect("at least one candidate")
}

fn category_has_unseen(
    category_members: &BTreeMap<String, Vec<usize>>,
    category: &str,
    unseen: &[bool],
) -> bool {
    category_members[category]
        .iter()
        .any(|index| unseen[*index])
}

fn recency_score<F>(history: &[RecentMessage], matches: F) -> usize
where
    F: Fn(&RecentMessage) -> bool,
{
    history.iter().rev().position(matches).unwrap_or(usize::MAX)
}

fn matches_recent_message(message: &MessageTemplate, recent: &RecentMessage) -> bool {
    message.text == recent.message && message.category == recent.category
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{IntervalMessage, MessageTemplate, ScheduledMessage, default_config};
    use chrono::NaiveTime;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn message(text: &str, category: &str) -> MessageTemplate {
        MessageTemplate::new(text, category)
    }

    #[test]
    fn generated_count_equals_pokes_per_day() {
        let config = default_config();
        let mut rng = StdRng::seed_from_u64(1);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        assert_eq!(pokes.len(), config.schedule.pokes_per_day);
    }

    #[test]
    fn generated_count_includes_scheduled_messages() {
        let mut config = default_config();
        config.scheduled.items = vec![
            ScheduledMessage::new(
                NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                "Before window.",
                "fixed",
            ),
            ScheduledMessage::new(
                NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
                "After window.",
                "fixed",
            ),
        ];
        let mut rng = StdRng::seed_from_u64(1);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            pokes.len(),
            config.schedule.pokes_per_day + config.scheduled.items.len()
        );
        assert_eq!(
            pokes
                .iter()
                .filter(|poke| poke.kind == PokeKind::Scheduled)
                .count(),
            2
        );
    }

    #[test]
    fn generated_count_includes_interval_messages() {
        let mut config = default_config();
        config.schedule.start_hour = 9;
        config.schedule.end_hour = 12;
        config.schedule.pokes_per_day = 1;
        config.intervals.items = vec![IntervalMessage::new(60, "Drink water.", "hydration")];
        let mut rng = StdRng::seed_from_u64(13);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        let interval_pokes: Vec<&PendingPoke> = pokes
            .iter()
            .filter(|poke| poke.kind == PokeKind::Interval)
            .collect();
        assert_eq!(interval_pokes.len(), 3);
        assert_eq!(
            pokes.len(),
            config.schedule.pokes_per_day + interval_pokes.len()
        );
        assert_eq!(interval_pokes[0].at.hour(), 9);
        assert_eq!(interval_pokes[1].at.hour(), 10);
        assert_eq!(interval_pokes[2].at.hour(), 11);
    }

    #[test]
    fn generated_times_are_inside_active_window() {
        let config = default_config();
        let date = NaiveDate::from_ymd_opt(2026, 4, 19).unwrap();
        let mut rng = StdRng::seed_from_u64(2);
        let interval =
            active_interval(date, config.schedule.start_hour, config.schedule.end_hour).unwrap();
        let pokes = generate_for_date(&config, &[], date, &mut rng).unwrap();
        assert!(
            pokes
                .iter()
                .all(|poke| poke.at >= interval.start && poke.at < interval.end)
        );
    }

    #[test]
    fn generated_times_respect_minimum_spacing() {
        let mut config = default_config();
        config.schedule.pokes_per_day = 4;
        config.schedule.min_spacing_minutes = 90;
        let mut rng = StdRng::seed_from_u64(3);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        for pair in pokes.windows(2) {
            assert!(pair[1].at - pair[0].at >= Duration::minutes(90));
        }
    }

    #[test]
    fn scheduled_messages_do_not_affect_minimum_spacing() {
        let mut config = default_config();
        config.schedule.pokes_per_day = 1;
        config.schedule.min_spacing_minutes = 120;
        config.scheduled.items = vec![
            ScheduledMessage::new(
                NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
                "First fixed.",
                "fixed",
            ),
            ScheduledMessage::new(
                NaiveTime::from_hms_opt(15, 1, 0).unwrap(),
                "Second fixed.",
                "fixed",
            ),
        ];
        let mut rng = StdRng::seed_from_u64(5);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            pokes
                .iter()
                .filter(|poke| poke.kind == PokeKind::Scheduled)
                .count(),
            2
        );
    }

    #[test]
    fn interval_messages_are_start_inclusive_and_end_exclusive() {
        let mut config = default_config();
        config.schedule.start_hour = 9;
        config.schedule.end_hour = 11;
        config.schedule.pokes_per_day = 1;
        config.intervals.items = vec![IntervalMessage::new(30, "Drink water.", "hydration")];
        let mut rng = StdRng::seed_from_u64(17);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        let times: Vec<String> = pokes
            .iter()
            .filter(|poke| poke.kind == PokeKind::Interval)
            .map(|poke| poke.at.format("%H:%M").to_string())
            .collect();
        assert_eq!(times, vec!["09:00", "09:30", "10:00", "10:30"]);
    }

    #[test]
    fn interval_larger_than_window_generates_start_only() {
        let mut config = default_config();
        config.schedule.start_hour = 9;
        config.schedule.end_hour = 10;
        config.schedule.pokes_per_day = 1;
        config.intervals.items = vec![IntervalMessage::new(120, "Drink water.", "hydration")];
        let mut rng = StdRng::seed_from_u64(19);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        let interval_pokes: Vec<&PendingPoke> = pokes
            .iter()
            .filter(|poke| poke.kind == PokeKind::Interval)
            .collect();
        assert_eq!(interval_pokes.len(), 1);
        assert_eq!(interval_pokes[0].at.format("%H:%M").to_string(), "09:00");
    }

    #[test]
    fn interval_messages_do_not_affect_minimum_spacing() {
        let mut config = default_config();
        config.schedule.pokes_per_day = 1;
        config.schedule.min_spacing_minutes = 120;
        config.intervals.items = vec![IntervalMessage::new(5, "Drink water.", "hydration")];
        let mut rng = StdRng::seed_from_u64(23);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        assert!(pokes.iter().any(|poke| poke.kind == PokeKind::Interval
            && poke.at.format("%H:%M").to_string() == "09:00"));
    }

    #[test]
    fn merged_pending_queue_is_sorted() {
        let mut config = default_config();
        config.schedule.start_hour = 9;
        config.schedule.end_hour = 12;
        config.schedule.pokes_per_day = 1;
        config.scheduled.items = vec![ScheduledMessage::new(
            NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            "Early fixed.",
            "fixed",
        )];
        config.intervals.items = vec![IntervalMessage::new(60, "Drink water.", "hydration")];
        let mut rng = StdRng::seed_from_u64(29);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        assert!(pokes.windows(2).all(|pair| pair[0].at <= pair[1].at));
    }

    #[test]
    fn all_messages_appear_when_pokes_exceed_message_count() {
        let config = default_config();
        // default: 5 messages, 6 pokes/day
        assert!(config.schedule.pokes_per_day >= config.messages.items.len());
        let mut rng = StdRng::seed_from_u64(42);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        let selected: std::collections::HashSet<&str> =
            pokes.iter().map(|p| p.message.as_str()).collect();
        for msg in &config.messages.items {
            assert!(
                selected.contains(msg.text.as_str()),
                "message not selected: {}",
                msg.text
            );
        }
    }

    #[test]
    fn active_window_is_start_inclusive_and_end_exclusive() {
        let start = DateTime::parse_from_rfc3339("2026-04-19T09:00:00-04:00").unwrap();
        let before_end = DateTime::parse_from_rfc3339("2026-04-19T20:59:59-04:00").unwrap();
        let end = DateTime::parse_from_rfc3339("2026-04-19T21:00:00-04:00").unwrap();
        assert!(is_within_active_window(start, 9, 21));
        assert!(is_within_active_window(before_end, 9, 21));
        assert!(!is_within_active_window(end, 9, 21));
    }

    #[test]
    fn avoids_consecutive_categories_when_alternatives_exist() {
        let mut config = default_config();
        config.schedule.pokes_per_day = 6;
        config.messages.items = vec![
            message("Drink water.", "hydration"),
            message("Stretch.", "movement"),
            message("Walk.", "movement"),
            message("Review notes.", "focus"),
        ];
        let mut rng = StdRng::seed_from_u64(7);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();

        for pair in pokes.windows(2) {
            let distinct_categories_exist = config
                .messages
                .items
                .iter()
                .any(|item| item.category != pair[0].category);
            if distinct_categories_exist {
                assert_ne!(pair[0].category, pair[1].category);
            }
        }
    }

    #[test]
    fn avoids_consecutive_messages_when_alternatives_exist() {
        let mut config = default_config();
        config.schedule.pokes_per_day = 5;
        config.messages.items = vec![
            message("Drink water.", "default"),
            message("Stretch.", "default"),
            message("Walk.", "default"),
        ];
        let mut rng = StdRng::seed_from_u64(9);
        let pokes = generate_for_date(
            &config,
            &[],
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();

        for pair in pokes.windows(2) {
            assert_ne!(pair[0].message, pair[1].message);
        }
    }

    #[test]
    fn recent_history_biases_the_first_category_of_the_day() {
        let mut config = default_config();
        config.schedule.pokes_per_day = 2;
        config.messages.items = vec![
            message("Drink water.", "hydration"),
            message("Stand up.", "movement"),
        ];
        let history = vec![RecentMessage::new("Drink water.", "hydration")];
        let mut rng = StdRng::seed_from_u64(11);
        let pokes = generate_for_date(
            &config,
            &history,
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();

        assert_eq!(pokes[0].category, "movement");
    }
}
