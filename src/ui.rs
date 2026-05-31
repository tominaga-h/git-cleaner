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
    /// 通常削除（git branch -d）。
    Delete,
    /// 強制削除（git branch -D）。未 push コミットがあるブランチ向け。
    Force,
    /// 削除せず次へ。
    Skip,
}

/// 候補ブランチ1件について削除可否を尋ねる。
///
/// - `y` / `yes` → `Delete`
/// - `n` / `no` / 空（デフォルト N）/ `skip` / `s` → `Skip`
/// - `force` / `f` → `Force`（未 push コミットがある候補でのみ受理）
/// - それ以外 → 再プロンプト
///
/// IO を注入することで TTY なしに単体テストできる。EOF（入力打ち切り）は
/// デフォルトの `Skip` として扱う。未 push コミットがある候補では選択肢に
/// `force` を加えて提示する。
pub fn prompt(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    candidate: &Candidate,
) -> Result<Decision> {
    let choices = if candidate.unpushed {
        "(y/N/skip/force)"
    } else {
        "(y/N/skip)"
    };
    loop {
        write!(
            writer,
            "? このローカルブランチを削除しますか？ {choices} > "
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
            "force" | "f" if candidate.unpushed => return Ok(Decision::Force),
            other => {
                writeln!(
                    writer,
                    "  '{other}' は無効な入力です。{choices} のいずれかを入力してください。"
                )?;
            }
        }
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

    // 見出し行: 完全マージか部分マージかで文言を変える。
    let heading = if c.partially_merged {
        format!(
            "[{idx}/{total}] ⚠ ブランチ '{name}' は '{target}' に一部マージ済みですが、未マージのコミットがあります。",
            name = c.name,
            target = c.matched_target,
        )
    } else {
        format!(
            "[{idx}/{total}] ブランチ '{name}' は '{target}' にマージ済みです。",
            name = c.name,
            target = c.matched_target,
        )
    };

    let merge_line = match &c.merge_info {
        Some(info) => {
            let merged_dt = info.merged_at.format("%Y-%m-%d %H:%M");
            let merged_rel = relative_time(info.merged_at, now);
            format!(
                "  - マージ: {merged_dt} ({merged_rel}) [{hash}]",
                hash = info.short_hash
            )
        }
        None if c.partially_merged => {
            "  - マージ: 一部のみ（その後のコミットは 'develop' 等に未取り込み）".to_string()
        }
        None => "  - マージ: 日時不明（マージコミットを特定できませんでした）".to_string(),
    };

    // 最終コミット行: 日時（相対）に続けてサマリ（メッセージ1行目）を併記。
    let last_commit_line = if c.last_commit_summary.is_empty() {
        format!("  - 最終コミット: {dt} ({rel})")
    } else {
        format!(
            "  - 最終コミット: {dt} ({rel}) {summary}",
            summary = c.last_commit_summary
        )
    };

    let mut out = format!(
        "{heading}\n{merge_line}\n{last_commit_line}\n  - リモート状態: {remote}",
        remote = remote_state_label(c.remote_state),
    );
    if c.partially_merged {
        out.push_str("\n  - 注意: 削除すると未マージのコミットを失う恐れがあります（git branch -d は安全のため拒否する場合があります）。");
    }
    if c.unpushed {
        out.push_str(
            "\n  - ⚠ 未 push: リモート(origin/同名)へ未反映のコミットがあります。\n    マージ先には入っていますが、git branch -d は拒否します。削除するなら force を選んでください。",
        );
    }
    out
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
            last_commit_summary: "fix: something".to_string(),
            remote_state: RemoteState::Alive,
            merge_info: None,
            partially_merged: false,
            unpushed: false,
        }
    }

    fn decide(input: &str) -> Decision {
        let mut reader = std::io::BufReader::new(input.as_bytes());
        let mut writer: Vec<u8> = Vec::new();
        prompt(&mut reader, &mut writer, &sample_candidate()).unwrap()
    }

    /// unpushed=true の候補に対してプロンプトする。
    fn decide_unpushed(input: &str) -> Decision {
        let mut c = sample_candidate();
        c.unpushed = true;
        let mut reader = std::io::BufReader::new(input.as_bytes());
        let mut writer: Vec<u8> = Vec::new();
        prompt(&mut reader, &mut writer, &c).unwrap()
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
            last_commit_summary: "implement login UI".to_string(),
            remote_state: RemoteState::Deleted,
            merge_info: Some(crate::git::MergeInfo {
                short_hash: "3f9a1c2".to_string(),
                merged_at: at(2026, 5, 26, 10, 0),
            }),
            partially_merged: false,
            unpushed: false,
        };
        let out = render_candidate(1, 2, &c, now);
        assert!(out.contains("[1/2]"));
        assert!(out.contains("feature/login-ui"));
        assert!(out.contains("develop"));
        assert!(out.contains("2026-05-25 14:30"));
        assert!(out.contains("6日前"));
        assert!(out.contains("削除済み"));
        // マージ行: 日時 + 短縮ハッシュ。
        assert!(out.contains("マージ: 2026-05-26 10:00"));
        assert!(out.contains("[3f9a1c2]"));
        // 最終コミット行にサマリ（メッセージ1行目）が併記される。
        assert!(
            out.contains("implement login UI"),
            "サマリが出るべき\n{out}"
        );
    }

    #[test]
    fn render_candidate_without_merge_info_shows_unknown() {
        let now = at(2026, 5, 31, 12, 0);
        let c = Candidate {
            name: "feature/x".to_string(),
            matched_target: "develop".to_string(),
            last_commit: at(2026, 5, 25, 14, 30),
            last_commit_summary: "wip".to_string(),
            remote_state: RemoteState::Unknown,
            merge_info: None,
            partially_merged: false,
            unpushed: false,
        };
        let out = render_candidate(1, 1, &c, now);
        assert!(
            out.contains("マージ: 日時不明"),
            "不明表示があるべき\n{out}"
        );
    }

    #[test]
    fn render_candidate_partially_merged_shows_warning() {
        let now = at(2026, 5, 31, 12, 0);
        let c = Candidate {
            name: "feature/wip".to_string(),
            matched_target: "develop".to_string(),
            last_commit: at(2026, 5, 30, 9, 0),
            last_commit_summary: "wip work".to_string(),
            remote_state: RemoteState::Alive,
            merge_info: None,
            partially_merged: true,
            unpushed: false,
        };
        let out = render_candidate(1, 1, &c, now);
        assert!(out.contains("⚠"), "警告マークがあるべき\n{out}");
        assert!(
            out.contains("未マージのコミット"),
            "警告文言があるべき\n{out}"
        );
        assert!(out.contains("失う恐れ"), "注意書きがあるべき\n{out}");
    }

    #[test]
    fn prompt_force_only_accepted_when_unpushed() {
        // unpushed=true なら force / f を受理。
        assert_eq!(decide_unpushed("force\n"), Decision::Force);
        assert_eq!(decide_unpushed("f\n"), Decision::Force);
        // y / n は従来通り。
        assert_eq!(decide_unpushed("y\n"), Decision::Delete);
        assert_eq!(decide_unpushed("n\n"), Decision::Skip);
    }

    #[test]
    fn prompt_force_rejected_when_not_unpushed() {
        // 未 push でない候補では force は無効入力扱い→再プロンプト後 y で Delete。
        let mut c = sample_candidate();
        c.unpushed = false;
        let mut reader = std::io::BufReader::new(&b"force\ny\n"[..]);
        let mut writer: Vec<u8> = Vec::new();
        let decision = prompt(&mut reader, &mut writer, &c).unwrap();
        assert_eq!(decision, Decision::Delete);
        let out = String::from_utf8(writer).unwrap();
        assert!(
            out.contains("無効な入力"),
            "force は受理されないべき\n{out}"
        );
        // 選択肢の提示は (y/N/skip) で force を含まない（無効入力メッセージ中の
        // 'force' という語ではなく、選択肢の提示文字列で確認する）。
        assert!(
            !out.contains("(y/N/skip/force)"),
            "選択肢に force を出さないべき\n{out}"
        );
    }

    #[test]
    fn prompt_offers_force_in_choices_when_unpushed() {
        let mut c = sample_candidate();
        c.unpushed = true;
        let mut reader = std::io::BufReader::new(&b"skip\n"[..]);
        let mut writer: Vec<u8> = Vec::new();
        prompt(&mut reader, &mut writer, &c).unwrap();
        let out = String::from_utf8(writer).unwrap();
        assert!(out.contains("force"), "選択肢に force を出すべき\n{out}");
    }

    #[test]
    fn render_candidate_unpushed_shows_warning() {
        let now = at(2026, 5, 31, 12, 0);
        let mut c = sample_candidate();
        c.unpushed = true;
        c.matched_target = "develop".to_string();
        let out = render_candidate(1, 1, &c, now);
        assert!(out.contains("未 push"), "未 push 警告が出るべき\n{out}");
        assert!(out.contains("force"), "force を促すべき\n{out}");
    }
}
