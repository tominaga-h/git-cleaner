//! git2 ファサード。リポジトリ操作はすべてこのモジュールを窓口にする。
//!
//! 検査系（ブランチ列挙・merged 判定・コミット日時）は git2 で実装し、
//! 副作用系（fetch --prune / branch -d）は後続タスクで `git` バイナリへ
//! シェルアウトする。

use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone};
use git2::{BranchType, Oid, Repository};
use std::process::Command;

/// 候補ブランチに対応するリモートブランチの生存状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteState {
    /// リモート追跡ブランチが残っている。
    Alive,
    /// upstream は設定されているが、prune によりリモート側が消えている。
    Deleted,
    /// upstream 未設定など、生存状態を判定できない。
    Unknown,
}

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
    Repository::discover(".").map_err(|_| {
        anyhow::anyhow!("ここは git リポジトリではありません。リポジトリ内で実行してください。")
    })
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

/// マージコミット1件の情報（取り込んだ日付と短縮ハッシュ）。
#[derive(Debug, Clone)]
pub struct MergeInfo {
    /// マージコミットの短縮ハッシュ。
    pub short_hash: String,
    /// マージコミットの作成日時（＝取り込んだ日付）。
    pub merged_at: DateTime<Local>,
}

/// ターゲット履歴を1回だけ走査し、各 `branch_tip` を「取り込んだ」マージコミット
/// を引けるマップを構築する。
///
/// `--no-ff` マージでは、ブランチ tip を親（通常は第2親）に持つマージコミットが
/// ターゲット側に作られる。ここでは「親のいずれかが対象 tip と一致するマージ
/// コミット（複数親）」のうち、ターゲット履歴上で最初に出会ったものを採用する。
/// fast-forward 等でマージコミットが存在しないブランチはマップに含まれない
/// （呼び出し側で「マージ日不明」として扱う）。
pub fn build_merge_info_map(
    repo: &Repository,
    target_tip: Oid,
    branch_tips: &[Oid],
) -> Result<std::collections::HashMap<Oid, MergeInfo>> {
    use std::collections::{HashMap, HashSet};

    let wanted: HashSet<Oid> = branch_tips.iter().copied().collect();
    let mut found: HashMap<Oid, MergeInfo> = HashMap::new();

    let mut walk = repo.revwalk().context("revwalk の作成に失敗しました")?;
    walk.push(target_tip)
        .context("revwalk の起点設定に失敗しました")?;

    for oid in walk {
        let oid = oid.context("revwalk の走査に失敗しました")?;
        let commit = repo
            .find_commit(oid)
            .context("コミットの取得に失敗しました")?;
        // マージコミット（親が2つ以上）のみが「取り込み」を表す。
        if commit.parent_count() < 2 {
            continue;
        }
        for parent_oid in commit.parent_ids() {
            // 既に確定済みの tip は上書きしない（最初に出会ったものを優先）。
            if wanted.contains(&parent_oid) && !found.contains_key(&parent_oid) {
                found.insert(
                    parent_oid,
                    MergeInfo {
                        short_hash: short_hash(&commit),
                        merged_at: commit_local_time(commit.time().seconds()),
                    },
                );
            }
        }
        // 全部見つかったら早期終了。
        if found.len() == wanted.len() {
            break;
        }
    }

    Ok(found)
}

/// コミットの短縮ハッシュ（git 既定の短縮長を尊重）を返す。
fn short_hash(commit: &git2::Commit) -> String {
    match commit.as_object().short_id() {
        Ok(buf) => buf.as_str().unwrap_or("").to_string(),
        // 取得失敗時は先頭7桁にフォールバック。
        Err(_) => commit.id().to_string().chars().take(7).collect(),
    }
}

/// `git fetch --prune` をシェルアウトで実行する。
///
/// 認証（SSH agent / HTTPS credential / known_hosts）はユーザーの `git` 設定を
/// そのまま流用するため、libgit2 の credential コールバックを書かずに済む。
pub fn fetch_prune(workdir: &std::path::Path) -> Result<()> {
    let output = Command::new("git")
        .args(["fetch", "--prune"])
        .current_dir(workdir)
        .output()
        .context("git fetch --prune の起動に失敗しました")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git fetch --prune に失敗しました: {}", stderr.trim());
    }
    Ok(())
}

/// ローカルブランチに対応するリモートブランチの生存状態を判定する。
///
/// - upstream が解決できる → `Alive`
/// - upstream 名は config 上存在するが ref が解決できない（prune 済み）→ `Deleted`
/// - upstream 未設定 → `Unknown`
pub fn remote_branch_alive(repo: &Repository, branch_name: &str) -> Result<RemoteState> {
    let branch = match repo.find_branch(branch_name, BranchType::Local) {
        Ok(b) => b,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(RemoteState::Unknown),
        Err(e) => return Err(e).with_context(|| format!("ブランチ '{branch_name}' の取得に失敗")),
    };

    // upstream（remote-tracking ref）が解決できれば生存。
    match branch.upstream() {
        Ok(_) => Ok(RemoteState::Alive),
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // config に upstream 名が登録されているかで「prune 済み」か「未追跡」を区別。
            let refname = format!("refs/heads/{branch_name}");
            match repo.branch_upstream_name(&refname) {
                Ok(_) => Ok(RemoteState::Deleted),
                Err(_) => Ok(RemoteState::Unknown),
            }
        }
        Err(e) => {
            Err(e).with_context(|| format!("ブランチ '{branch_name}' の upstream 解決に失敗"))
        }
    }
}

/// `git branch -d <name>` をシェルアウトで実行する（安全削除）。
///
/// libgit2 の `Branch::delete` ではなく git の `-d` を使うことで、git 本来の
/// 「マージ済みでなければ拒否する」安全セマンティクスとエラーメッセージを
/// そのまま踏襲する。失敗時は git の stderr を surface する。
pub fn delete_branch(workdir: &std::path::Path, name: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "-d", name])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("git branch -d {name} の起動に失敗しました"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("'{name}' の削除に失敗しました: {}", stderr.trim());
    }
    Ok(())
}

/// Unix 秒をローカルタイムゾーンの日時へ変換する。
fn commit_local_time(seconds: i64) -> DateTime<Local> {
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .unwrap_or_else(|| Local.timestamp_opt(0, 0).single().expect("epoch is valid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// upstream を持たないブランチは Unknown。
    #[test]
    fn remote_alive_unknown_without_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "T"]);
        std::fs::write(p.join("a.txt"), "a").unwrap();
        git(p, &["add", "a.txt"]);
        git(p, &["commit", "-q", "-m", "init"]);

        let repo = Repository::open(p).unwrap();
        assert_eq!(
            remote_branch_alive(&repo, "main").unwrap(),
            RemoteState::Unknown
        );
    }

    /// 存在しないブランチ名も Unknown。
    #[test]
    fn remote_alive_unknown_for_missing_branch() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);

        let repo = Repository::open(p).unwrap();
        assert_eq!(
            remote_branch_alive(&repo, "does-not-exist").unwrap(),
            RemoteState::Unknown
        );
    }

    /// upstream を持つブランチは Alive。
    #[test]
    fn remote_alive_when_upstream_present() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        // bare リモートを用意。
        let remote = p.join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        // クローンして push、upstream を設定。
        let work = p.join("work");
        git(p, &["clone", "-q", remote.to_str().unwrap(), "work"]);
        git(&work, &["config", "user.email", "t@e.com"]);
        git(&work, &["config", "user.name", "T"]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        git(&work, &["add", "a.txt"]);
        git(&work, &["commit", "-q", "-m", "init"]);
        git(&work, &["push", "-q", "-u", "origin", "main"]);

        let repo = Repository::open(&work).unwrap();
        assert_eq!(
            remote_branch_alive(&repo, "main").unwrap(),
            RemoteState::Alive
        );
    }
}
