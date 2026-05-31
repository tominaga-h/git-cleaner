//! 表示整形と対話プロンプト。
//!
//! Task 1 では候補の整形表示（`render_candidate` / `relative_time`）のみ。
//! 対話プロンプト（y/N/skip）は Task 4 で追加する。

use crate::cleaner::Candidate;
use crate::git::RemoteState;
use anyhow::Result;
use chrono::{DateTime, Local};
use std::io::{BufRead, Write};

/// 1ブランチに対するユーザーの判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 削除する。
    Delete,
    /// 削除せず次へ。
    Skip,
}

/// 候補ブランチ1件について y/N/skip を尋ねる。
///
/// - `y` / `yes` → `Delete`
/// - `n` / `no` / 空（デフォルト N）/ `skip` / `s` → `Skip`
/// - それ以外 → 再プロンプト
///
/// IO を注入することで TTY なしに単体テストできる。EOF（入力打ち切り）は
/// デフォルトの `Skip` として扱う。
pub fn prompt(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    candidate: &Candidate,
) -> Result<Decision> {
    loop {
        write!(
            writer,
            "? このローカルブランチを削除しますか？ (y/N/skip) > "
        )?;
        writer.flush()?;

        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            // EOF: デフォルト（Skip）。
            return Ok(Decision::Skip);
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(Decision::Delete),
            "" | "n" | "no" | "skip" | "s" => return Ok(Decision::Skip),
            other => {
                writeln!(
                    writer,
                    "  '{other}' は無効な入力です。y / N / skip を入力してください。"
                )?;
            }
        }
        // candidate は将来 UI 拡張で使う可能性があるため引数に保持。
        let _ = candidate;
    }
}

/// リモート生存状態を表示用の日本語文字列に変換する。
fn remote_state_label(state: RemoteState) -> &'static str {
    match state {
        RemoteState::Alive => "存在 (リモートにもまだ残っています)",
        RemoteState::Deleted => "削除済み (リモートで削除されています)",
        RemoteState::Unknown => "不明 (リモート未追跡)",
    }
}

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
        "[{idx}/{total}] ブランチ '{name}' は '{target}' にマージ済みです。\n  - 最終コミット: {dt} ({rel})\n  - リモート状態: {remote}",
        name = c.name,
        target = c.matched_target,
        remote = remote_state_label(c.remote_state),
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

    fn sample_candidate() -> Candidate {
        Candidate {
            name: "feature/x".to_string(),
            matched_target: "main".to_string(),
            last_commit: at(2026, 5, 25, 14, 30),
            remote_state: RemoteState::Alive,
        }
    }

    fn decide(input: &str) -> Decision {
        let mut reader = std::io::BufReader::new(input.as_bytes());
        let mut writer: Vec<u8> = Vec::new();
        prompt(&mut reader, &mut writer, &sample_candidate()).unwrap()
    }

    #[test]
    fn prompt_y_is_delete() {
        assert_eq!(decide("y\n"), Decision::Delete);
        assert_eq!(decide("yes\n"), Decision::Delete);
        assert_eq!(decide("Y\n"), Decision::Delete);
    }

    #[test]
    fn prompt_n_and_empty_and_skip_are_skip() {
        assert_eq!(decide("n\n"), Decision::Skip);
        assert_eq!(decide("\n"), Decision::Skip); // 空入力=デフォルトN
        assert_eq!(decide("skip\n"), Decision::Skip);
        assert_eq!(decide("s\n"), Decision::Skip);
    }

    #[test]
    fn prompt_eof_is_skip() {
        assert_eq!(decide(""), Decision::Skip);
    }

    #[test]
    fn prompt_reprompts_on_invalid_then_accepts() {
        // 無効入力 "maybe" の後に "y" を与えると最終的に Delete。
        let mut reader = std::io::BufReader::new(&b"maybe\ny\n"[..]);
        let mut writer: Vec<u8> = Vec::new();
        let decision = prompt(&mut reader, &mut writer, &sample_candidate()).unwrap();
        assert_eq!(decision, Decision::Delete);
        let out = String::from_utf8(writer).unwrap();
        assert!(out.contains("無効な入力"), "再プロンプトされるべき\n{out}");
    }

    #[test]
    fn render_candidate_contains_name_target_and_date() {
        let now = at(2026, 5, 31, 12, 0);
        let c = Candidate {
            name: "feature/login-ui".to_string(),
            matched_target: "develop".to_string(),
            last_commit: at(2026, 5, 25, 14, 30),
            remote_state: RemoteState::Deleted,
        };
        let out = render_candidate(1, 2, &c, now);
        assert!(out.contains("[1/2]"));
        assert!(out.contains("feature/login-ui"));
        assert!(out.contains("develop"));
        assert!(out.contains("2026-05-25 14:30"));
        assert!(out.contains("6日前"));
        assert!(out.contains("削除済み"));
    }
}
