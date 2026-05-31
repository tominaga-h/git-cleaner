//! git2 ファサード。リポジトリ操作はすべてこのモジュールを窓口にする。
//!
//! 検査系（ブランチ列挙・merged 判定・コミット日時）は git2 で実装し、
//! 副作用系（fetch --prune / branch -d）は後続タスクで `git` バイナリへ
//! シェルアウトする。

use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone};
use git2::{BranchType, Oid, Repository};

/// ローカルブランチ1件のメタ情報。
#[derive(Debug, Clone)]
pub struct LocalBranch {
    /// ブランチ名（`refs/heads/` を除いた短縮名）。
    pub name: String,
    /// ブランチ tip のコミット OID。
    pub tip: Oid,
    /// tip コミットの作成日時（ローカルタイムゾーン）。
    pub last_commit_time: DateTime<Local>,
}

/// カレントディレクトリ起点でリポジトリを探索して開く。
pub fn open() -> Result<Repository> {
    Repository::discover(".").context("git リポジトリが見つかりません")
}

/// 現在チェックアウトしているブランチ名（detached HEAD 等では `None`）。
pub fn current_branch(repo: &Repository) -> Result<Option<String>> {
    let head = match repo.head() {
        Ok(head) => head,
        // 空リポジトリ（unborn branch）など HEAD が解決できない場合。
        Err(_) => return Ok(None),
    };
    if !head.is_branch() {
        return Ok(None);
    }
    Ok(head.shorthand().ok().map(|s| s.to_string()))
}

/// ローカルブランチを列挙し、tip OID とコミット日時を添えて返す。
pub fn local_branches(repo: &Repository) -> Result<Vec<LocalBranch>> {
    let mut result = Vec::new();
    for entry in repo
        .branches(Some(BranchType::Local))
        .context("ローカルブランチの列挙に失敗しました")?
    {
        let (branch, _) = entry.context("ブランチ情報の取得に失敗しました")?;
        let name = match branch.name()? {
            Some(name) => name.to_string(),
            None => continue, // 非 UTF-8 名はスキップ。
        };
        let reference = branch.get();
        let tip = reference
            .target()
            .with_context(|| format!("ブランチ '{name}' の tip を解決できません"))?;
        let commit = repo
            .find_commit(tip)
            .with_context(|| format!("ブランチ '{name}' のコミットを取得できません"))?;
        let last_commit_time = commit_local_time(commit.time().seconds());
        result.push(LocalBranch {
            name,
            tip,
            last_commit_time,
        });
    }
    Ok(result)
}

/// ブランチ名（ローカル）から tip OID を解決する。存在しなければ `None`。
pub fn resolve_target_tip(repo: &Repository, name: &str) -> Result<Option<Oid>> {
    match repo.find_branch(name, BranchType::Local) {
        Ok(branch) => Ok(branch.get().target()),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("ブランチ '{name}' の解決に失敗しました")),
    }
}

/// `branch_tip` が `target_tip` の履歴に含まれるか（＝通常マージ済みか）。
///
/// `graph_descendant_of(target, branch)` は target が branch の子孫であること、
/// すなわち target の履歴に branch tip が含まれることを意味する。同一コミット
/// （fast-forward 済み）の場合も「マージ済み」とみなす。
pub fn is_merged_into(repo: &Repository, branch_tip: Oid, target_tip: Oid) -> Result<bool> {
    if branch_tip == target_tip {
        return Ok(true);
    }
    repo.graph_descendant_of(target_tip, branch_tip)
        .context("マージ判定に失敗しました")
}

/// Unix 秒をローカルタイムゾーンの日時へ変換する。
fn commit_local_time(seconds: i64) -> DateTime<Local> {
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .unwrap_or_else(|| Local.timestamp_opt(0, 0).single().expect("epoch is valid"))
}
