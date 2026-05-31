//! マージ済みブランチ掃除動作のオーケストレーション。
//!
//! Task 1: repo を開き、ローカルブランチを列挙し、ターゲットへのマージ済み
//! 判定を行い、カレントブランチを除外して候補を表示する（dry-run コア）。
//! config 読込（Task 2）・fetch/リモート生存（Task 3）・対話削除（Task 4）は後続。

use crate::git;
use crate::ui;
use anyhow::{bail, Result};
use chrono::{DateTime, Local};

/// 削除候補ブランチ1件。`find_candidates` が組み立て、UI / 削除で消費する。
#[derive(Debug, Clone)]
pub struct Candidate {
    /// ブランチ名。
    pub name: String,
    /// マージ済みと判定されたターゲットブランチ名。
    pub matched_target: String,
    /// 最終コミット日時。
    pub last_commit: DateTime<Local>,
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
}

/// マージ済みブランチ掃除のエントリポイント。
pub fn run(dry_run: bool, target: Option<String>) -> Result<()> {
    let repo = git::open()?;
    let current = git::current_branch(&repo)?;

    // Task 1 暫定: config 未対応のため、ターゲットは `--target` 指定必須。
    // Task 2 で `cleaner.targets` からの読み込みに置き換える。
    let targets: Vec<String> = match target {
        Some(t) => vec![t],
        None => {
            bail!("ターゲットブランチを -t/--target で指定してください（config 対応は Task 2）")
        }
    };

    // 各ローカルブランチについて、最初にマージ済みと判定できたターゲットを記録。
    let branches = git::local_branches(&repo)?;
    let mut merged = Vec::with_capacity(branches.len());
    for b in branches {
        let mut matched_target = None;
        for t in &targets {
            let Some(target_tip) = git::resolve_target_tip(&repo, t)? else {
                continue; // 存在しないターゲットは無視。
            };
            if git::is_merged_into(&repo, b.tip, target_tip)? {
                matched_target = Some(t.clone());
                break;
            }
        }
        merged.push(MergedBranch {
            name: b.name,
            matched_target,
            last_commit: b.last_commit_time,
        });
    }

    let candidates = find_candidates(merged, current.as_deref(), &[]);

    if candidates.is_empty() {
        println!("削除対象のブランチはありません。");
        return Ok(());
    }

    let now = Local::now();
    let total = candidates.len();
    for (i, c) in candidates.iter().enumerate() {
        println!("{}", ui::render_candidate(i + 1, total, c, now));
    }

    if dry_run {
        println!("\n（dry-run: 削除は行いません）");
    }
    // Task 4 で対話削除をここに追加する。
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
