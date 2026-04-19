use crate::config::Config;
use crate::state::PendingPoke;
use anyhow::{Context, bail};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, NaiveDate, TimeZone, Timelike,
};
use rand::Rng;

const MAX_ATTEMPTS: usize = 1_000;

pub fn generate_for_date<R: Rng + ?Sized>(
    config: &Config,
    date: NaiveDate,
    rng: &mut R,
) -> anyhow::Result<Vec<PendingPoke>> {
    let interval = active_interval(date, config.schedule.start_hour, config.schedule.end_hour)?;
    generate_in_interval(config, date, interval, rng)
}

fn generate_in_interval<R: Rng + ?Sized>(
    config: &Config,
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
            return Ok(times
                .into_iter()
                .enumerate()
                .map(|(index, at)| PendingPoke {
                    id: format!("{date}-{index}"),
                    at,
                    message: random_message(&config.messages.items, rng),
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

fn local_datetime(date: NaiveDate, hour: u32) -> anyhow::Result<DateTime<FixedOffset>> {
    let local = match Local.with_ymd_and_hms(date.year(), date.month(), date.day(), hour, 0, 0) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => bail!("local time {date} {hour:02}:00 does not exist"),
    };
    Ok(local.fixed_offset())
}

fn respects_min_spacing(times: &[DateTime<FixedOffset>], min_spacing: Duration) -> bool {
    times
        .windows(2)
        .all(|pair| pair[1] - pair[0] >= min_spacing)
}

fn random_message<R: Rng + ?Sized>(messages: &[String], rng: &mut R) -> String {
    let index = rng.gen_range(0..messages.len());
    messages[index].clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn generated_count_equals_pokes_per_day() {
        let config = default_config();
        let mut rng = StdRng::seed_from_u64(1);
        let pokes = generate_for_date(
            &config,
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        assert_eq!(pokes.len(), config.schedule.pokes_per_day);
    }

    #[test]
    fn generated_times_are_inside_active_window() {
        let config = default_config();
        let date = NaiveDate::from_ymd_opt(2026, 4, 19).unwrap();
        let mut rng = StdRng::seed_from_u64(2);
        let interval =
            active_interval(date, config.schedule.start_hour, config.schedule.end_hour).unwrap();
        let pokes = generate_for_date(&config, date, &mut rng).unwrap();
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
            NaiveDate::from_ymd_opt(2026, 4, 19).unwrap(),
            &mut rng,
        )
        .unwrap();
        for pair in pokes.windows(2) {
            assert!(pair[1].at - pair[0].at >= Duration::minutes(90));
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
}
