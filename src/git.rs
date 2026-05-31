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
    /// tip コミットメッセージの1行目（サマリ）。
    pub last_commit_summary: String,
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
        // コミットメッセージの1行目（サマリ）。非 UTF-8 / 取得失敗時は空文字。
        let last_commit_summary = commit.summary().ok().flatten().unwrap_or("").to_string();
        result.push(LocalBranch {
            name,
            tip,
            last_commit_time,
            last_commit_summary,
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

/// ブランチがターゲットへどの程度マージされているか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStatus {
    /// ブランチ tip がターゲットに完全に含まれている（安全に削除可能）。
    Fully,
    /// 過去にターゲットへマージされた形跡はあるが、その後にターゲット未取り込みの
    /// コミットがブランチに追加されている（削除すると未マージ分を失う恐れ）。
    Partially,
    /// ターゲットにマージされた形跡がない。
    NotMerged,
}

/// ブランチ tip とターゲット tip から、マージ状態を判定する。
///
/// 判定方法:
/// - tip がターゲットに含まれる → `Fully`
/// - tip は含まれないが、merge-base がブランチ独自のコミット（ターゲットに
///   取り込まれた過去のブランチ作業）になっている → `Partially`
/// - merge-base がブランチの分岐元そのもの（取り込まれた作業が無い）→ `NotMerged`
///
/// 判定の核心は「merge-base がブランチ固有コミットか、共有の分岐元か」を見ること。
/// 部分マージでは、ターゲットへ取り込まれたブランチの最終コミットが merge-base に
/// 前進する。これを「merge-base がブランチ tip の親方向に進んだ別コミットか」では
/// なく、「ターゲット履歴にこのブランチを取り込んだマージコミットがあるか」で判定
/// する（最も曖昧さが少ない）。
pub fn merge_status(repo: &Repository, branch_tip: Oid, target_tip: Oid) -> Result<MergeStatus> {
    if is_merged_into(repo, branch_tip, target_tip)? {
        return Ok(MergeStatus::Fully);
    }

    // 共通祖先（merge-base）。無ければ無関係な履歴＝未マージ。
    let base = match repo.merge_base(branch_tip, target_tip) {
        Ok(b) => b,
        Err(_) => return Ok(MergeStatus::NotMerged),
    };

    // ターゲットの「第1親のみ」の本流履歴を辿る。merge-base が本流上にあれば、
    // それは分岐元（共有コミット）であり、このブランチの作業は取り込まれていない
    // ＝未マージ。本流上に無ければ、merge-base はマージ（第2親）経由でのみ
    // ターゲットに入った＝このブランチの作業が過去に取り込まれた＝部分マージ。
    //
    // 例: 部分マージでは merge-base が「取り込まれたブランチの最終コミット」になり、
    // それはターゲットの第1親本流には乗らない。develop から分岐しただけの未マージ
    // ブランチでは merge-base が develop 本流上のコミットになる。
    let mut commit = repo
        .find_commit(target_tip)
        .context("ターゲットコミットの取得に失敗しました")?;
    loop {
        if commit.id() == base {
            // 分岐元が本流上にある＝未マージ。
            return Ok(MergeStatus::NotMerged);
        }
        match commit.parent(0) {
            Ok(first_parent) => commit = first_parent,
            Err(_) => break, // 根に到達（base が本流に無かった）。
        }
    }

    // base が第1親本流に現れなかった＝マージ経由で取り込まれた＝部分マージ。
    Ok(MergeStatus::Partially)
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

/// ローカルブランチに、upstream（リモート追跡ブランチ）へ未 push のコミットが
/// あるかを判定する。
///
/// `git branch -d` は upstream が設定されていると「upstream にマージ済みか」を
/// 安全基準にするため、ターゲット（develop 等）にマージ済みでも upstream に未 push
/// のコミットがあると削除を拒否する。これを事前に検知して警告するために使う。
///
/// 判定: upstream が解決でき、かつ `branch_tip` が upstream tip と異なり、
/// upstream tip が `branch_tip` の子孫でない（= ローカルに upstream へ未反映の
/// コミットがある）場合に true。upstream 未設定や解決不能時は false。
pub fn has_unpushed_commits(repo: &Repository, branch_name: &str) -> Result<bool> {
    let branch = match repo.find_branch(branch_name, BranchType::Local) {
        Ok(b) => b,
        Err(_) => return Ok(false),
    };
    let local_tip = match branch.get().target() {
        Some(t) => t,
        None => return Ok(false),
    };
    let upstream = match branch.upstream() {
        Ok(u) => u,
        // upstream 未設定/prune 済み → ここでは「未 push」とは扱わない。
        Err(_) => return Ok(false),
    };
    let upstream_tip = match upstream.get().target() {
        Some(t) => t,
        None => return Ok(false),
    };
    if local_tip == upstream_tip {
        return Ok(false);
    }
    // upstream tip が local tip を含む（= ローカルは upstream に追いついている/遅れて
    // いる）なら未 push なし。含まないなら、ローカルに upstream 未反映のコミットあり。
    let upstream_contains_local = repo
        .graph_descendant_of(upstream_tip, local_tip)
        .unwrap_or(false);
    Ok(!upstream_contains_local)
}

/// `git branch -d`（force=false）/ `git branch -D`（force=true）をシェルアウトする。
///
/// `-d` は git 本来の安全削除（マージ済みでなければ拒否）。`-D` は強制削除。
/// 失敗時は git の stderr を surface する。
pub fn delete_branch(workdir: &std::path::Path, name: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    let output = Command::new("git")
        .args(["branch", flag, name])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("git branch {flag} {name} の起動に失敗しました"))?;
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

    fn commit(dir: &Path, file: &str, msg: &str) {
        std::fs::write(dir.join(file), msg).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", msg]);
    }

    fn tip(repo: &Repository, branch: &str) -> Oid {
        repo.find_branch(branch, BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap()
    }

    /// merge_status: 完全マージ / 部分マージ / 未マージ を判定できる。
    #[test]
    fn merge_status_distinguishes_full_partial_notmerged() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "develop"]);
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "T"]);
        commit(p, "a.txt", "init");

        // feature/full: develop にマージ後、追加コミットなし → Fully。
        git(p, &["checkout", "-q", "-b", "feature/full"]);
        commit(p, "f.txt", "full work");
        git(p, &["checkout", "-q", "develop"]);
        git(
            p,
            &["merge", "-q", "--no-ff", "-m", "merge full", "feature/full"],
        );

        // feature/partial: develop にマージ後、さらにブランチ側へ追加コミット → Partially。
        git(p, &["checkout", "-q", "-b", "feature/partial"]);
        commit(p, "g.txt", "partial work");
        git(p, &["checkout", "-q", "develop"]);
        git(
            p,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "merge partial",
                "feature/partial",
            ],
        );
        git(p, &["checkout", "-q", "feature/partial"]);
        commit(p, "h.txt", "extra unmerged work");

        // feature/none: develop の最新から分岐したが未マージ → NotMerged。
        git(p, &["checkout", "-q", "develop"]);
        git(p, &["checkout", "-q", "-b", "feature/none"]);
        commit(p, "i.txt", "unmerged");

        // 初期コミット(init)から分岐した古い未マージブランチも NotMerged。
        let init_oid = {
            let out = Command::new("git")
                .args(["rev-list", "--max-parents=0", "develop"])
                .current_dir(p)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        git(p, &["checkout", "-q", "-b", "feature/old", &init_oid]);
        commit(p, "j.txt", "old unmerged");

        let repo = Repository::open(p).unwrap();
        let dev = tip(&repo, "develop");

        assert_eq!(
            merge_status(&repo, tip(&repo, "feature/full"), dev).unwrap(),
            MergeStatus::Fully
        );
        assert_eq!(
            merge_status(&repo, tip(&repo, "feature/partial"), dev).unwrap(),
            MergeStatus::Partially
        );
        assert_eq!(
            merge_status(&repo, tip(&repo, "feature/none"), dev).unwrap(),
            MergeStatus::NotMerged
        );
        assert_eq!(
            merge_status(&repo, tip(&repo, "feature/old"), dev).unwrap(),
            MergeStatus::NotMerged,
            "古い分岐点の未マージブランチも NotMerged"
        );
    }

    /// has_unpushed_commits: ローカルに upstream 未反映のコミットがあると true。
    #[test]
    fn has_unpushed_detects_local_ahead_of_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let remote = p.join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        let work = p.join("work");
        git(p, &["clone", "-q", remote.to_str().unwrap(), "work"]);
        git(&work, &["config", "user.email", "t@e.com"]);
        git(&work, &["config", "user.name", "T"]);

        // feature を作って push（upstream 設定）→ この時点では未 push なし。
        git(&work, &["checkout", "-q", "-b", "feature/x"]);
        commit(&work, "a.txt", "a");
        git(&work, &["push", "-q", "-u", "origin", "feature/x"]);
        {
            let repo = Repository::open(&work).unwrap();
            assert!(
                !has_unpushed_commits(&repo, "feature/x").unwrap(),
                "push 直後は未 push なし"
            );
        }

        // ローカルに追加コミット（push しない）→ 未 push あり。
        commit(&work, "b.txt", "b");
        {
            let repo = Repository::open(&work).unwrap();
            assert!(
                has_unpushed_commits(&repo, "feature/x").unwrap(),
                "push していないコミットがあれば true"
            );
        }
    }

    /// upstream 未設定のブランチは未 push 判定の対象外（false）。
    #[test]
    fn has_unpushed_false_without_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "T"]);
        commit(p, "a.txt", "a");
        git(p, &["checkout", "-q", "-b", "feature/local"]);
        commit(p, "b.txt", "b");

        let repo = Repository::open(p).unwrap();
        assert!(!has_unpushed_commits(&repo, "feature/local").unwrap());
    }
}
