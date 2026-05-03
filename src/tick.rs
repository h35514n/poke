use crate::config::Config;
use crate::delivery::{DeliveryOutput, ImsgSender, Sender};
use crate::paths::PokePaths;
use crate::schedule;
use crate::state::{self, PendingPoke, PokeKind, RecentMessage, SentPoke, State, StateLock};
use anyhow::{Context, bail};
use chrono::{DateTime, FixedOffset, Local};

const RECENT_HISTORY_LIMIT: usize = 8;

pub fn run_tick(paths: &PokePaths) -> anyhow::Result<()> {
    paths.ensure_dirs()?;
    let _lock = StateLock::acquire(&paths.lock_file)?;
    let config = Config::load(&paths.config_file)?;
    let mut state = state::load_state(&paths.state_file)?;
    let now = Local::now().fixed_offset();
    let mut sender = ImsgSender::new(&config.delivery);
    let outcome = process_tick(&config, &mut state, now, &mut sender)?;
    if outcome.state_changed {
        state::save_state_atomic(&paths.state_file, &state)?;
    }
    Ok(())
}

pub fn regen_today(paths: &PokePaths) -> anyhow::Result<()> {
    paths.ensure_dirs()?;
    let _lock = StateLock::acquire(&paths.lock_file)?;
    let config = Config::load(&paths.config_file)?;
    let mut state = state::load_state(&paths.state_file)?;
    regenerate_today(&config, &mut state, Local::now().fixed_offset())?;
    state::save_state_atomic(&paths.state_file, &state)?;
    Ok(())
}

pub fn show(paths: &PokePaths) -> anyhow::Result<String> {
    let state = state::load_state(&paths.state_file)?;
    let mut output = String::new();
    output.push_str(&format!("config: {}\n", paths.config_file.display()));
    output.push_str(&format!("state: {}\n", paths.state_file.display()));
    output.push_str(&format!("logs: {}\n", paths.log_dir.display()));
    output.push_str(&format!(
        "last_schedule_date: {:?}\n",
        state.last_schedule_date
    ));
    output.push_str("pending:\n");
    for poke in &state.pending {
        output.push_str(&format!(
            "  {} {} {} [{}] {}\n",
            poke.id,
            poke.at,
            poke.kind.as_str(),
            poke.category,
            poke.message
        ));
    }
    output.push_str("last_sent:\n");
    if let Some(sent) = state.sent.last() {
        output.push_str(&format!(
            "  {} scheduled={} sent={} {} [{}] {}\n",
            sent.id,
            sent.scheduled_at,
            sent.sent_at,
            sent.kind.as_str(),
            sent.category,
            sent.message
        ));
    } else {
        output.push_str("  none\n");
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickOutcome {
    pub state_changed: bool,
    pub sent_message: bool,
}

pub fn process_tick<S: Sender>(
    config: &Config,
    state: &mut State,
    now: DateTime<FixedOffset>,
    sender: &mut S,
) -> anyhow::Result<TickOutcome> {
    let mut state_changed = false;
    if state.last_schedule_date != Some(now.date_naive()) {
        regenerate_today(config, state, now)?;
        state_changed = true;
    }

    let within_active_window = schedule::is_within_active_window(
        now,
        config.schedule.start_hour,
        config.schedule.end_hour,
    );

    let Some((due_index, due)) = earliest_due(&state.pending, now, within_active_window) else {
        return Ok(TickOutcome {
            state_changed,
            sent_message: false,
        });
    };
    let delivery = sender.send(&due.message)?;
    if delivery.status_code != Some(0) {
        log_delivery_failure(&delivery);
        bail!(
            "imsg failed with exit status {}",
            delivery
                .status_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "terminated by signal".to_string())
        );
    }

    let sent_poke = state.pending.remove(due_index);
    state.pending.retain(|poke| poke.at > now);
    let recent_message = RecentMessage::new(sent_poke.message.clone(), sent_poke.category.clone());
    state.sent.push(SentPoke {
        id: sent_poke.id,
        scheduled_at: sent_poke.at,
        sent_at: now,
        message: sent_poke.message,
        category: sent_poke.category.clone(),
        kind: sent_poke.kind,
    });
    if sent_poke.kind == PokeKind::Random {
        record_recent_send(&mut state.recent_history, recent_message);
    }
    Ok(TickOutcome {
        state_changed: true,
        sent_message: true,
    })
}

pub fn regenerate_today(
    config: &Config,
    state: &mut State,
    now: DateTime<FixedOffset>,
) -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();
    let pending =
        schedule::generate_for_date(config, &state.recent_history, now.date_naive(), &mut rng)
            .context("failed to generate today's schedule")?;
    state.last_schedule_date = Some(now.date_naive());
    state.pending = pending;
    state.sent.clear();
    Ok(())
}

fn earliest_due(
    pending: &[PendingPoke],
    now: DateTime<FixedOffset>,
    within_active_window: bool,
) -> Option<(usize, PendingPoke)> {
    pending
        .iter()
        .enumerate()
        .find(|(_, poke)| {
            poke.at <= now && (poke.kind == PokeKind::Scheduled || within_active_window)
        })
        .map(|(index, poke)| (index, poke.clone()))
}

fn log_delivery_failure(output: &DeliveryOutput) {
    eprintln!("imsg exit status: {:?}", output.status_code);
    if !output.stdout.trim().is_empty() {
        eprintln!("imsg stdout: {}", output.stdout.trim_end());
    }
    if !output.stderr.trim().is_empty() {
        eprintln!("imsg stderr: {}", output.stderr.trim_end());
    }
}

fn record_recent_send(history: &mut Vec<RecentMessage>, sent: RecentMessage) {
    history.push(sent);
    if history.len() > RECENT_HISTORY_LIMIT {
        let excess = history.len() - RECENT_HISTORY_LIMIT;
        history.drain(0..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;
    use crate::delivery::DeliveryOutput;
    use crate::paths::PokePaths;
    use crate::state::save_state_atomic;
    use chrono::{DateTime, Duration, FixedOffset, NaiveDate};

    struct FakeSender {
        status_code: Option<i32>,
        calls: Vec<String>,
    }

    impl Sender for FakeSender {
        fn send(&mut self, message: &str) -> anyhow::Result<DeliveryOutput> {
            self.calls.push(message.to_string());
            Ok(DeliveryOutput {
                status_code: self.status_code,
                stdout: String::new(),
                stderr: "boom".to_string(),
            })
        }
    }

    fn dt(hour: i32, minute: i32) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(&format!("2026-04-19T{hour:02}:{minute:02}:00-04:00")).unwrap()
    }

    fn poke(id: &str, at: DateTime<FixedOffset>) -> PendingPoke {
        poke_with_kind(id, at, PokeKind::Random)
    }

    fn poke_with_kind(id: &str, at: DateTime<FixedOffset>, kind: PokeKind) -> PendingPoke {
        PendingPoke {
            id: id.to_string(),
            at,
            message: format!("message {id}"),
            category: "default".to_string(),
            kind,
        }
    }

    #[test]
    fn no_op_before_first_due_time() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![poke("a", dt(10, 0))],
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        let outcome = process_tick(&config, &mut state, dt(9, 30), &mut sender).unwrap();
        assert!(!outcome.state_changed);
        assert!(!outcome.sent_message);
        assert!(sender.calls.is_empty());
        assert_eq!(state.pending.len(), 1);
    }

    #[test]
    fn due_poke_is_dequeued_after_success() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![poke("a", dt(9, 0)), poke("b", dt(10, 0))],
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        let outcome = process_tick(&config, &mut state, dt(9, 5), &mut sender).unwrap();
        assert!(outcome.state_changed);
        assert!(outcome.sent_message);
        assert_eq!(sender.calls, vec!["message a"]);
        assert_eq!(state.pending, vec![poke("b", dt(10, 0))]);
        assert_eq!(state.sent.len(), 1);
        assert_eq!(
            state.recent_history,
            vec![RecentMessage::new("message a", "default")]
        );
    }

    #[test]
    fn multiple_overdue_sends_one_and_drops_other_missed_after_success() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![
                poke("a", dt(9, 0)),
                poke("b", dt(9, 30)),
                poke("c", dt(11, 0)),
            ],
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        process_tick(&config, &mut state, dt(10, 0), &mut sender).unwrap();
        assert_eq!(sender.calls, vec!["message a"]);
        assert_eq!(state.pending, vec![poke("c", dt(11, 0))]);
        assert_eq!(state.sent.len(), 1);
    }

    #[test]
    fn failed_send_preserves_pending_queue() {
        let config = default_config();
        let original = vec![poke("a", dt(9, 0)), poke("b", dt(10, 0))];
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: original.clone(),
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(1),
            calls: vec![],
        };
        assert!(process_tick(&config, &mut state, dt(9, 5), &mut sender).is_err());
        assert_eq!(state.pending, original);
        assert!(state.sent.is_empty());
        assert!(state.recent_history.is_empty());
    }

    #[test]
    fn new_day_rollover_replaces_pending_and_clears_sent() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 18).unwrap()),
            pending: vec![poke("old", dt(9, 0))],
            sent: vec![SentPoke {
                id: "old".to_string(),
                scheduled_at: dt(9, 0) - Duration::days(1),
                sent_at: dt(9, 5) - Duration::days(1),
                message: "old".to_string(),
                category: "default".to_string(),
                kind: PokeKind::Random,
            }],
            recent_history: vec![RecentMessage::new("old", "default")],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        let outcome = process_tick(&config, &mut state, dt(8, 0), &mut sender).unwrap();
        assert!(outcome.state_changed);
        assert!(!outcome.sent_message);
        assert_eq!(
            state.last_schedule_date,
            Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap())
        );
        assert_eq!(state.pending.len(), config.schedule.pokes_per_day);
        assert!(state.sent.is_empty());
        assert_eq!(
            state.recent_history,
            vec![RecentMessage::new("old", "default")]
        );
    }

    #[test]
    fn show_output_does_not_mutate_state() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PokePaths::from_bases(temp.path().join("config"), temp.path().join("state"));
        let state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![poke("a", dt(9, 0))],
            sent: vec![],
            recent_history: vec![],
        };
        save_state_atomic(&paths.state_file, &state).unwrap();
        let before = std::fs::read_to_string(&paths.state_file).unwrap();

        let output = show(&paths).unwrap();

        let after = std::fs::read_to_string(&paths.state_file).unwrap();
        assert!(output.contains("pending:"));
        assert_eq!(before, after);
    }

    #[test]
    fn recent_history_is_bounded_to_last_successful_sends() {
        let mut history = Vec::new();
        for index in 0..10 {
            record_recent_send(
                &mut history,
                RecentMessage::new(format!("message {index}"), "default"),
            );
        }

        assert_eq!(history.len(), RECENT_HISTORY_LIMIT);
        assert_eq!(history.first().unwrap().message, "message 2");
        assert_eq!(history.last().unwrap().message, "message 9");
    }

    #[test]
    fn random_poke_does_not_send_after_active_window() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![poke("a", dt(20, 0))],
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        let outcome = process_tick(&config, &mut state, dt(21, 30), &mut sender).unwrap();
        assert!(!outcome.state_changed);
        assert!(!outcome.sent_message);
        assert!(sender.calls.is_empty());
        assert_eq!(state.pending, vec![poke("a", dt(20, 0))]);
    }

    #[test]
    fn scheduled_poke_sends_after_active_window() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![poke_with_kind("fixed", dt(21, 15), PokeKind::Scheduled)],
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        let outcome = process_tick(&config, &mut state, dt(21, 30), &mut sender).unwrap();
        assert!(outcome.state_changed);
        assert!(outcome.sent_message);
        assert_eq!(sender.calls, vec!["message fixed"]);
        assert!(state.pending.is_empty());
        assert_eq!(state.sent[0].kind, PokeKind::Scheduled);
        assert!(state.recent_history.is_empty());
    }

    #[test]
    fn earliest_due_still_limits_tick_to_one_message() {
        let config = default_config();
        let mut state = State {
            last_schedule_date: Some(NaiveDate::from_ymd_opt(2026, 4, 19).unwrap()),
            pending: vec![
                poke_with_kind("fixed", dt(9, 0), PokeKind::Scheduled),
                poke("random", dt(9, 5)),
            ],
            sent: vec![],
            recent_history: vec![],
        };
        let mut sender = FakeSender {
            status_code: Some(0),
            calls: vec![],
        };
        process_tick(&config, &mut state, dt(9, 10), &mut sender).unwrap();
        assert_eq!(sender.calls, vec!["message fixed"]);
        assert!(state.pending.is_empty());
        assert_eq!(state.sent.len(), 1);
    }
}
