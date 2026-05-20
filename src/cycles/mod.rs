mod accounts;
mod cli;
mod formatters;
mod reports;
mod store;
mod time;
mod usage;
mod windows;

pub use cli::{
    run_cycle_add, run_cycle_current, run_cycle_history, run_cycle_list, run_cycle_remove,
    CycleCommandHelps, CycleCommandOptions,
};
pub use store::WeeklyCycleAnchor;

use crate::error::AppError;

pub(super) const WEEKLY_CYCLE_STORE_VERSION: u8 = 1;
pub(super) const WEEKLY_CYCLE_PERIOD_HOURS: i64 = 168;
pub(super) const WEEKLY_CYCLE_PERIOD_MS: i64 = WEEKLY_CYCLE_PERIOD_HOURS * 60 * 60 * 1000;
pub(super) const DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID: &str = "default";

fn normalize_required_id(value: &str, label: &str) -> Result<String, AppError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        Err(AppError::new(format!(
            "Weekly cycle {label} cannot be empty."
        )))
    } else {
        Ok(normalized.to_string())
    }
}

fn normalize_optional_id(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::stats::{StatFormat, TokenUsage, UsageRecord};

    #[test]
    fn parses_multiple_cycle_add_times() {
        assert_eq!(
            time::parse_cycle_add_times(&[
                "2026-05-17".to_string(),
                "09:00".to_string(),
                "2026-05-18T00:00:00Z,2026-05-19".to_string()
            ])
            .expect("times"),
            vec!["2026-05-17 09:00", "2026-05-18T00:00:00Z", "2026-05-19"]
        );
    }

    #[test]
    fn derives_delayed_weekly_cycles() {
        let anchors = vec![anchor("anchor-may-01", "2026-05-01T00:00:00.000Z")];
        let records = vec![
            record("2026-05-01T01:00:00.000Z", "session-a", 100),
            record("2026-05-07T23:59:59.000Z", "session-a", 20),
            record("2026-05-09T08:00:00.000Z", "session-b", 50),
        ];
        let report = reports::build_weekly_cycle_history_report(
            &anchors,
            records,
            None,
            time::parse_iso_timestamp("2026-05-10T00:00:00.000Z").expect("now"),
            false,
            None,
        );

        assert_eq!(report.status, "ok");
        assert_eq!(
            report.rows.iter().map(|row| row.source).collect::<Vec<_>>(),
            vec!["manual", "derived"]
        );
        assert_eq!(
            report
                .rows
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["anchor-may-01", "cyc_20260509T080000000Z"]
        );
        assert_eq!(report.rows[0].calls, 2);
        assert_eq!(report.rows[1].usage.total_tokens, 50);
        assert_eq!(report.totals.calls, 3);
    }

    #[test]
    fn estimates_windows_before_anchor_when_requested() {
        let anchors = vec![anchor("anchor-may-08", "2026-05-08T00:00:00.000Z")];
        let records = vec![
            record("2026-05-01T01:00:00.000Z", "session-a", 100),
            record("2026-05-08T01:00:00.000Z", "session-b", 50),
        ];
        let report = reports::build_weekly_cycle_history_report(
            &anchors,
            records,
            Some(time::parse_iso_timestamp("2026-05-01T00:00:00.000Z").expect("start")),
            time::parse_iso_timestamp("2026-05-10T00:00:00.000Z").expect("end"),
            true,
            None,
        );

        assert_eq!(
            report.rows.iter().map(|row| row.source).collect::<Vec<_>>(),
            vec!["estimated", "manual"]
        );
        assert_eq!(report.diagnostics.estimated_windows, 1);
        assert_eq!(report.diagnostics.ignored_before_anchor_events, 0);
    }

    #[test]
    fn unpriced_model_notes_are_carried_in_cycle_totals() {
        let anchors = vec![anchor("anchor-may-01", "2026-05-01T00:00:00.000Z")];
        let mut unpriced = record("2026-05-01T01:00:00.000Z", "session-a", 100);
        unpriced.model = "new-unknown-model".to_string();
        let report = reports::build_weekly_cycle_history_report(
            &anchors,
            vec![unpriced],
            None,
            time::parse_iso_timestamp("2026-05-02T00:00:00.000Z").expect("end"),
            false,
            None,
        );

        assert_eq!(report.totals.unpriced_calls, 1);
        assert_eq!(report.totals.unpriced_models[0].model, "new-unknown-model");
        assert!(report.totals.unpriced_models[0]
            .pricing_stub
            .contains("\"new-unknown-model\""));
    }

    #[test]
    fn interactive_history_select_formats_selected_detail() {
        let anchors = vec![anchor("anchor-may-01", "2026-05-01T00:00:00.000Z")];
        let records = vec![
            record("2026-05-01T01:00:00.000Z", "session-a", 100),
            record("2026-05-09T08:00:00.000Z", "session-b", 50),
        ];
        let history = reports::build_weekly_cycle_history_report(
            &anchors,
            records.clone(),
            None,
            time::parse_iso_timestamp("2026-05-10T00:00:00.000Z").expect("now"),
            false,
            None,
        );
        let context = reports::WeeklyCycleReportContext {
            account_id: Some("account-fixture".to_string()),
            account_label: None,
            account_source: Some(accounts::WeeklyCycleAccountSource::Explicit.as_str()),
            cycle_file: Some("/tmp/stat-cycles.json".to_string()),
        };
        let mut prompt = FakePrompt {
            select: Some(Some(1)),
            ..FakePrompt::default()
        };

        let output = cli::select_weekly_cycle_history_detail(
            &history,
            records,
            None,
            StatFormat::Table,
            &context,
            &mut prompt,
        )
        .expect("selected detail");

        assert!(output.contains("Codex weekly cycle detail"));
        assert!(output.contains("Cycle ID: cyc_20260509T080000000Z"));
        assert!(output.contains("50"));
        assert_eq!(prompt.select_items[0].len(), 2);
        assert!(prompt.select_items[0][1].contains("cyc_20260509T080000000Z"));
    }

    #[derive(Default)]
    struct FakePrompt {
        select: Option<Option<usize>>,
        select_items: Vec<Vec<String>>,
    }

    impl crate::prompt::Prompt for FakePrompt {
        fn select(&mut self, _prompt: &str, items: &[String]) -> Result<Option<usize>, AppError> {
            self.select_items.push(items.to_vec());
            self.select
                .take()
                .ok_or_else(|| AppError::new("missing fake select response"))
        }

        fn multi_select(
            &mut self,
            _prompt: &str,
            _items: &[String],
        ) -> Result<Option<Vec<usize>>, AppError> {
            Err(AppError::new("unexpected fake multi-select call"))
        }

        fn confirm(&mut self, _prompt: &str, _default: bool) -> Result<Option<bool>, AppError> {
            Err(AppError::new("unexpected fake confirm call"))
        }
    }

    fn anchor(id: &str, at: &str) -> WeeklyCycleAnchor {
        WeeklyCycleAnchor {
            id: id.to_string(),
            at: at.to_string(),
            input: at.to_string(),
            time_zone: "UTC".to_string(),
            source: "manual".to_string(),
            note: String::new(),
            created_at: "2026-05-01T00:00:00.000Z".to_string(),
        }
    }

    fn record(timestamp: &str, session_id: &str, total_tokens: i64) -> UsageRecord {
        UsageRecord {
            timestamp: time::parse_iso_timestamp(timestamp).expect("timestamp"),
            session_id: session_id.to_string(),
            model: "gpt-5.5".to_string(),
            reasoning_effort: None,
            cwd: "/repo".to_string(),
            account_id: Some("account-fixture".to_string()),
            file_path: "/tmp/session.jsonl".to_string(),
            usage: TokenUsage {
                input_tokens: total_tokens,
                cached_input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens,
            },
        }
    }
}
