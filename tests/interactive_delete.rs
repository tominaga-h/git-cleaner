//! 統合テスト: 対話削除（y で削除 / n で残存 / --dry-run は削除なし）。

use assert_cmd::Command;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;

fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    git(dir.path(), &["config", "cleaner.targets", "main"]);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn commit_file(dir: &Path, file: &str, content: &str, msg: &str) {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// main にマージ済みの feature/x を持つ repo を作る（カレントは main）。
fn repo_with_merged_feature() -> TempDir {
    let repo = init_repo();
    let p = repo.path();
    commit_file(p, "a.txt", "a", "init");
    git(p, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(p, "b.txt", "b", "feature work");
    git(p, &["checkout", "-q", "main"]);
    git(p, &["merge", "-q", "--no-ff", "-m", "merge", "feature/x"]);
    repo
}

/// ローカルブランチが存在するか。
fn branch_exists(dir: &Path, name: &str) -> bool {
    StdCommand::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{name}")])
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn answering_yes_deletes_branch() {
    let repo = repo_with_merged_feature();
    let p = repo.path();
    assert!(branch_exists(p, "feature/x"));

    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(p)
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("削除しました"));

    assert!(!branch_exists(p, "feature/x"), "y で削除されるべき");
}

#[test]
fn answering_no_keeps_branch() {
    let repo = repo_with_merged_feature();
    let p = repo.path();

    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(p)
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("スキップ"));

    assert!(branch_exists(p, "feature/x"), "n で残存すべき");
}

#[test]
fn dry_run_never_deletes_even_with_yes() {
    let repo = repo_with_merged_feature();
    let p = repo.path();

    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(p)
        .arg("-d")
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("dry-run"));

    assert!(
        branch_exists(p, "feature/x"),
        "--dry-run は入力に関わらず削除しないべき"
    );
}
