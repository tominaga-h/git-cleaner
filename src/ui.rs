//! 表示整形と対話プロンプト。
//!
//! Task 1 では候補の整形表示（`render_candidate` / `relative_time`）のみ。
//! 対話プロンプト（y/N/skip）は Task 4 で追加する。

use crate::cleaner::Candidate;
use chrono::{DateTime, Local};

/// `then` から `now` までの相対時間を日本語で返す（"今日" / "N日前" など）。
///
/// 未来日時（クロックずれ等）は "今日" に丸める。
pub fn relative_time(then: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (now.date_naive() - then.date_naive()).num_days();
    match days {
        d if d <= 0 => "今日".to_string(),
        1 => "昨日".to_string(),
        d => format!("{d}日前"),
    }
}

/// 候補ブランチ1件を `[i/total]` 形式の複数行テキストに整形する。
pub fn render_candidate(idx: usize, total: usize, c: &Candidate, now: DateTime<Local>) -> String {
    let dt = c.last_commit.format("%Y-%m-%d %H:%M");
    let rel = relative_time(c.last_commit, now);
    format!(
        "[{idx}/{total}] ブランチ '{name}' は '{target}' にマージ済みです。\n  - 最終コミット: {dt} ({rel})",
        name = c.name,
        target = c.matched_target,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    #[test]
    fn relative_time_same_day_is_today() {
        let now = at(2026, 5, 31, 12, 0);
        let then = at(2026, 5, 31, 9, 0);
        assert_eq!(relative_time(then, now), "今日");
    }

    #[test]
    fn relative_time_yesterday() {
        let now = at(2026, 5, 31, 12, 0);
        let then = at(2026, 5, 30, 23, 0);
        assert_eq!(relative_time(then, now), "昨日");
    }

    #[test]
    fn relative_time_n_days_ago() {
        let now = at(2026, 5, 31, 12, 0);
        let then = at(2026, 5, 25, 14, 30);
        assert_eq!(relative_time(then, now), "6日前");
    }

    #[test]
    fn relative_time_future_clamps_to_today() {
        let now = at(2026, 5, 31, 0, 0);
        let then = at(2026, 6, 1, 0, 0);
        assert_eq!(relative_time(then, now), "今日");
    }

    #[test]
    fn render_candidate_contains_name_target_and_date() {
        let now = at(2026, 5, 31, 12, 0);
        let c = Candidate {
            name: "feature/login-ui".to_string(),
            matched_target: "develop".to_string(),
            last_commit: at(2026, 5, 25, 14, 30),
        };
        let out = render_candidate(1, 2, &c, now);
        assert!(out.contains("[1/2]"));
        assert!(out.contains("feature/login-ui"));
        assert!(out.contains("develop"));
        assert!(out.contains("2026-05-25 14:30"));
        assert!(out.contains("6日前"));
    }
}
