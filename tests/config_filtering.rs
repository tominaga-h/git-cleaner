//! 統合テスト: config 読込（cleaner.targets / cleaner.protect）と --target 上書き。
//!
//! - `cleaner.targets` から自動でターゲットを読む（-t 不要）
//! - `cleaner.protect` に指定したブランチは merged でも候補に出ない
//! - `-t/--target` が config の targets を上書きする

use assert_cmd::Command;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;

fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed", args);
}

fn commit_file(dir: &Path, file: &str, content: &str, msg: &str) {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// `branch` を作りコミット→`base` に --no-ff マージして戻る（base がカレント）。
fn create_and_merge(dir: &Path, base: &str, branch: &str, file: &str) {
    git(dir, &["checkout", "-q", "-b", branch]);
    commit_file(dir, file, "x", &format!("{branch} work"));
    git(dir, &["checkout", "-q", base]);
    git(
        dir,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("merge {branch}"),
            branch,
        ],
    );
}

fn run(dir: &Path, args: &[&str]) -> String {
    let assert = Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(dir)
        .args(args)
        .assert()
        .success();
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

#[test]
fn reads_targets_from_config_without_t_flag() {
    let repo = init_repo();
    let p = repo.path();
    git(p, &["config", "cleaner.targets", "main"]);
    commit_file(p, "a.txt", "a", "init");
    create_and_merge(p, "main", "feature/a", "b.txt");

    // -t を渡さなくても cleaner.targets=main により feature/a が候補に出る。
    let out = run(p, &["-d"]);
    assert!(
        out.contains("feature/a"),
        "config の targets が使われるべき\n{out}"
    );
}

#[test]
fn protect_excludes_merged_branch() {
    let repo = init_repo();
    let p = repo.path();
    git(p, &["config", "cleaner.targets", "main"]);
    git(p, &["config", "cleaner.protect", "staging"]);
    commit_file(p, "a.txt", "a", "init");

    // staging と feature/a はどちらも main にマージ済み。
    create_and_merge(p, "main", "staging", "s.txt");
    create_and_merge(p, "main", "feature/a", "b.txt");

    let out = run(p, &["-d"]);
    assert!(
        out.contains("feature/a"),
        "未保護ブランチは候補に出るべき\n{out}"
    );
    assert!(
        !out.contains("ブランチ 'staging'"),
        "protect 指定ブランチは merged でも除外されるべき\n{out}"
    );
}

#[test]
fn target_flag_overrides_config_targets() {
    let repo = init_repo();
    let p = repo.path();
    // config では develop をターゲットにしているが、feature は main にのみマージ。
    git(p, &["config", "cleaner.targets", "develop"]);
    commit_file(p, "a.txt", "a", "init");
    // develop ブランチを作っておく（feature はそこにはマージしない）。
    git(p, &["branch", "develop"]);
    create_and_merge(p, "main", "feature/a", "b.txt");

    // -t main で上書きすれば feature/a が候補に出る（config の develop は無視）。
    let out = run(p, &["-d", "-t", "main"]);
    assert!(
        out.contains("feature/a"),
        "--target が config の targets を上書きすべき\n{out}"
    );
}
