//! マージ済みブランチ掃除動作のオーケストレーション。
//!
//! Task 1: repo を開き、ローカルブランチを列挙し、ターゲットへのマージ済み
//! 判定を行い、カレントブランチを除外して候補を表示する（dry-run コア）。
//! Task 2: `cleaner.targets` / `cleaner.protect` を config から読み、`--target`
//! でターゲットを上書き、保護ブランチとカレントを除外する。
//! fetch/リモート生存（Task 3）・対話削除（Task 4）は後続。

use crate::config;
use crate::git::{self, MergeInfo, MergeStatus, RemoteState};
use crate::ui;
use anyhow::{bail, Result};
use chrono::{DateTime, Local};
use std::io::Write;

/// 削除候補ブランチ1件。`find_candidates` が組み立て、UI / 削除で消費する。
#[derive(Debug, Clone)]
pub struct Candidate {
    /// ブランチ名。
    pub name: String,
    /// マージ済みと判定されたターゲットブランチ名。
    pub matched_target: String,
    /// 最終コミット日時。
    pub last_commit: DateTime<Local>,
    /// 最終コミットメッセージの1行目（サマリ）。
    pub last_commit_summary: String,
    /// 対応するリモートブランチの生存状態。
    pub remote_state: RemoteState,
    /// ターゲット側で取り込んだマージコミット情報（特定できない場合は `None`）。
    pub merge_info: Option<MergeInfo>,
    /// 過去にマージされたが、その後ターゲット未取り込みのコミットが追加されている。
    pub partially_merged: bool,
    /// upstream（origin/同名）へ未 push のローカルコミットがある（`-d` が拒否する）。
    pub unpushed: bool,
}

/// merged 判定済みの入力（1ブランチ + マッチしたターゲット）。
///
/// git2 への依存をここで切り、`find_candidates` を実 repo 不要で単体テスト
/// できるようにするための中間表現。`matched_target` が `None` のものは
/// どのターゲットにもマージされていない（候補外）。
#[derive(Debug, Clone)]
pub struct MergedBranch {
    pub name: String,
    pub matched_target: Option<String>,
    pub last_commit: DateTime<Local>,
    pub last_commit_summary: String,
    pub remote_state: RemoteState,
    /// ターゲット側で取り込んだマージコミット情報（特定できない場合は `None`）。
    pub merge_info: Option<MergeInfo>,
    /// 過去にマージされたが、その後ターゲット未取り込みのコミットが追加されている。
    pub partially_merged: bool,
    /// upstream（origin/同名）へ未 push のローカルコミットがある（`-d` が拒否する）。
    pub unpushed: bool,
}

/// マージ済みブランチ掃除のエントリポイント。
pub fn run(dry_run: bool, target: Option<String>, limit: Option<usize>) -> Result<()> {
    let repo = git::open()?;
    let workdir = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // 「裏側で」fetch --prune を実行する。ローカル分析と重ねるため先に開始し、
    // リモート生存判定の直前に完了を待つ。失敗しても警告のみで分析は継続する。
    let fetch_result = git::fetch_prune(&workdir);
    if let Err(e) = &fetch_result {
        eprintln!("warning: {e:#}（リモート生存状態は不明として続行します）");
    }
    let fetch_ok = fetch_result.is_ok();

    let current = git::current_branch(&repo)?;
    let cfg = config::load(&repo)?;

    // `--target` 指定があれば config の targets を上書き、なければ config を使う。
    let targets: Vec<String> = match target {
        Some(t) => vec![t],
        None => cfg.targets.clone(),
    };
    if targets.is_empty() {
        bail!(
            "マージ先ターゲットが設定されていません。`cleaner.targets` を設定するか -t/--target を指定してください。"
        );
    }

    // 各ローカルブランチについて、最初にマージ済みと判定できたターゲットを記録。
    // マージコミット特定はターゲット単位でまとめて行うため、ここでは tip と
    // マッチしたターゲット名だけを先に確定する。
    struct Pending {
        name: String,
        tip: git2::Oid,
        matched_target: Option<String>,
        partially_merged: bool,
        unpushed: bool,
        last_commit: DateTime<Local>,
        last_commit_summary: String,
        remote_state: RemoteState,
    }

    // ターゲット名 → tip OID（存在しないターゲットは除外）。
    let target_tips: Vec<(String, git2::Oid)> = targets
        .iter()
        .filter_map(|t| match git::resolve_target_tip(&repo, t) {
            Ok(Some(tip)) => Some(Ok((t.clone(), tip))),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_>>()?;

    let branches = git::local_branches(&repo)?;
    let mut pending: Vec<Pending> = Vec::with_capacity(branches.len());
    for b in branches {
        // 各ターゲットについてマージ状態を見て、最初に「完全/部分マージ」と判定
        // できたターゲットを採用する（完全マージを優先）。
        let mut matched_target = None;
        let mut partially_merged = false;
        for (name, tip) in &target_tips {
            match git::merge_status(&repo, b.tip, *tip)? {
                MergeStatus::Fully => {
                    matched_target = Some(name.clone());
                    partially_merged = false;
                    break;
                }
                MergeStatus::Partially => {
                    // 完全マージが他ターゲットで見つかる可能性があるので即 break せず、
                    // 暫定的に部分マージとして記録しておく。
                    if matched_target.is_none() {
                        matched_target = Some(name.clone());
                        partially_merged = true;
                    }
                }
                MergeStatus::NotMerged => {}
            }
        }
        // fetch に失敗した場合は生存状態を判定せず Unknown 扱い。
        let remote_state = if fetch_ok {
            git::remote_branch_alive(&repo, &b.name)?
        } else {
            RemoteState::Unknown
        };
        // upstream へ未 push のコミットがあるか（git branch -d が拒否する条件）。
        let unpushed = git::has_unpushed_commits(&repo, &b.name)?;
        pending.push(Pending {
            name: b.name,
            tip: b.tip,
            matched_target,
            partially_merged,
            unpushed,
            last_commit: b.last_commit_time,
            last_commit_summary: b.last_commit_summary,
            remote_state,
        });
    }

    // ターゲット単位で履歴を1回走査し、各ブランチ tip を取り込んだマージコミット
    // を引く。完全マージ（tip が親）のみマージコミットが特定できる。部分マージは
    // tip が移動しているため特定できず None（警告表示でカバー）。
    let mut merge_info_by_tip: std::collections::HashMap<git2::Oid, MergeInfo> =
        std::collections::HashMap::new();
    for (name, tip) in &target_tips {
        let tips_for_target: Vec<git2::Oid> = pending
            .iter()
            .filter(|p| !p.partially_merged && p.matched_target.as_deref() == Some(name.as_str()))
            .map(|p| p.tip)
            .collect();
        if tips_for_target.is_empty() {
            continue;
        }
        let map = git::build_merge_info_map(&repo, *tip, &tips_for_target)?;
        merge_info_by_tip.extend(map);
    }

    let merged: Vec<MergedBranch> = pending
        .into_iter()
        .map(|p| MergedBranch {
            merge_info: merge_info_by_tip.get(&p.tip).cloned(),
            name: p.name,
            matched_target: p.matched_target,
            last_commit: p.last_commit,
            last_commit_summary: p.last_commit_summary,
            remote_state: p.remote_state,
            partially_merged: p.partially_merged,
            unpushed: p.unpushed,
        })
        .collect();

    let mut candidates = find_candidates(merged, current.as_deref(), &cfg.protect);

    if candidates.is_empty() {
        println!("削除対象のブランチはありません。");
        return Ok(());
    }

    // --limit: 抽出後の先頭 n 件に絞る。truncate された件数は明示する。
    let matched = candidates.len();
    if let Some(n) = limit {
        if n < candidates.len() {
            candidates.truncate(n);
            println!("削除対象 {matched} 件のうち、先頭 {n} 件に絞り込みました（--limit {n}）。\n");
        }
    }

    let now = Local::now();
    let total = candidates.len();

    if dry_run {
        // dry-run: 一覧表示のみ。
        for (i, c) in candidates.iter().enumerate() {
            println!("{}", ui::render_candidate(i + 1, total, c, now));
        }
        println!("\n（dry-run: 削除は行いません）");
        return Ok(());
    }

    // 対話削除: 1件ずつ提示して y/N/skip を尋ね、Delete なら git branch -d。
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    for (i, c) in candidates.iter().enumerate() {
        writeln!(writer, "{}", ui::render_candidate(i + 1, total, c, now))?;
        match ui::prompt(&mut reader, &mut writer, c)? {
            // 通常削除（git branch -d）。git の安全機構が拒否すればそのまま surface。
            ui::Decision::Delete => match git::delete_branch(&workdir, &c.name, false) {
                Ok(()) => writeln!(writer, "-> 削除しました: {}", c.name)?,
                Err(e) => writeln!(writer, "-> {e:#}")?,
            },
            // 強制削除（git branch -D）。未 push コミットがあるブランチ向け。
            ui::Decision::Force => match git::delete_branch(&workdir, &c.name, true) {
                Ok(()) => writeln!(writer, "-> 強制削除しました: {}", c.name)?,
                Err(e) => writeln!(writer, "-> {e:#}")?,
            },
            ui::Decision::Skip => writeln!(writer, "-> スキップしました。")?,
        }
        // 候補ごとの区切りに空行を入れる。
        writeln!(writer)?;
    }
    Ok(())
}

/// merged 判定済みブランチから削除候補を抽出する（純粋関数・テストの要）。
///
/// 除外ルール:
/// - どのターゲットにもマージされていない（`matched_target` が `None`）
/// - カレントブランチ（`current`）
/// - 保護ブランチ（`protect`、Task 2 で配線）
pub fn find_candidates(
    branches: Vec<MergedBranch>,
    current: Option<&str>,
    protect: &[String],
) -> Vec<Candidate> {
    branches
        .into_iter()
        .filter_map(|b| {
            let matched_target = b.matched_target?;
            if current == Some(b.name.as_str()) {
                return None;
            }
            if protect.iter().any(|p| p == &b.name) {
                return None;
            }
            Some(Candidate {
                name: b.name,
                matched_target,
                last_commit: b.last_commit,
                last_commit_summary: b.last_commit_summary,
                remote_state: b.remote_state,
                merge_info: b.merge_info,
                partially_merged: b.partially_merged,
                unpushed: b.unpushed,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 5, 25, 14, 30, 0)
            .single()
            .unwrap()
    }

    fn branch(name: &str, target: Option<&str>) -> MergedBranch {
        MergedBranch {
            name: name.to_string(),
            matched_target: target.map(|s| s.to_string()),
            last_commit: ts(),
            last_commit_summary: "summary".to_string(),
            remote_state: RemoteState::Unknown,
            merge_info: None,
            partially_merged: false,
            unpushed: false,
        }
    }

    #[test]
    fn keeps_only_merged_branches() {
        let input = vec![
            branch("feature/a", Some("develop")),
            branch("feature/b", None),
        ];
        let out = find_candidates(input, None, &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "feature/a");
        assert_eq!(out[0].matched_target, "develop");
    }

    #[test]
    fn excludes_current_branch_even_if_merged() {
        let input = vec![
            branch("develop", Some("main")),
            branch("feature/a", Some("develop")),
        ];
        let out = find_candidates(input, Some("develop"), &[]);
        let names: Vec<_> = out.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["feature/a"]);
    }

    #[test]
    fn excludes_protected_branches() {
        let input = vec![
            branch("staging", Some("main")),
            branch("feature/a", Some("develop")),
        ];
        let protect = vec!["staging".to_string()];
        let out = find_candidates(input, None, &protect);
        let names: Vec<_> = out.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["feature/a"]);
    }

    #[test]
    fn empty_when_nothing_merged() {
        let input = vec![branch("feature/a", None), branch("feature/b", None)];
        let out = find_candidates(input, None, &[]);
        assert!(out.is_empty());
    }
}
