//! 統合テスト: エラーパスとメッセージ／exit code。
//!
//! - 非 git リポジトリ → エラー終了（exit != 0）
//! - targets 未設定 → エラー終了し設定を促すメッセージ
//! - 候補ゼロ → 正常終了（exit 0）で「対象なし」メッセージ

use assert_cmd::Command;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn repo_with_one_commit() -> TempDir {
    let dir = TempDir::new().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@e.com"]);
    git(p, &["config", "user.name", "T"]);
    std::fs::write(p.join("a.txt"), "a").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "init"]);
    dir
}

#[test]
fn not_a_git_repo_fails() {
    // git repo でない空ディレクトリ。
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(dir.path())
        .arg("-t")
        .arg("main")
        .assert()
        .failure()
        .stderr(predicates::str::contains("git リポジトリ"));
}

#[test]
fn missing_targets_fails_with_hint() {
    let repo = repo_with_one_commit();
    // cleaner.targets 未設定かつ -t なし。
    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("cleaner.targets"));
}

#[test]
fn no_candidates_succeeds_with_message() {
    let repo = repo_with_one_commit();
    // main しかなく、main はカレント（除外）なので候補ゼロ。
    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(repo.path())
        .args(["-t", "main"])
        .assert()
        .success()
        .stdout(predicates::str::contains("削除対象のブランチはありません"));
}
