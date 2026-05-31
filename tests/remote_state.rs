//! 統合テスト: fetch --prune 後のリモート生存状態表示。
//!
//! bare repo を origin として、ブランチを push→リモート側で削除→git-cleaner が
//! 内部で fetch --prune を実行し、候補の「削除済み」/「存在」表示を検証する。

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

fn commit_file(dir: &Path, file: &str, content: &str, msg: &str) {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// origin(bare) + clone(work) を構築し、main を push して返す。
fn setup() -> (TempDir, std::path::PathBuf) {
    let root = TempDir::new().unwrap();
    let p = root.path();
    let remote = p.join("remote.git");
    std::fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);

    git(p, &["clone", "-q", remote.to_str().unwrap(), "work"]);
    let work = p.join("work");
    git(&work, &["config", "user.email", "t@e.com"]);
    git(&work, &["config", "user.name", "T"]);
    git(&work, &["config", "cleaner.targets", "main"]);
    commit_file(&work, "a.txt", "a", "init");
    git(&work, &["push", "-q", "-u", "origin", "main"]);

    (root, work)
}

#[test]
fn merged_branch_shows_deleted_after_remote_prune() {
    let (_root, work) = setup();

    // feature を作り push（upstream 設定）→ main にマージ。
    git(&work, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(&work, "b.txt", "b", "feature work");
    git(&work, &["push", "-q", "-u", "origin", "feature/x"]);
    git(&work, &["checkout", "-q", "main"]);
    git(
        &work,
        &["merge", "-q", "--no-ff", "-m", "merge", "feature/x"],
    );

    // リモート側の feature/x を削除（GitHub 等でブランチ削除した状況を再現）。
    git(&work, &["push", "-q", "origin", "--delete", "feature/x"]);

    // git-cleaner 実行: 内部で fetch --prune が走り、削除済みと判定される。
    let assert = Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(&work)
        .arg("-d")
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(out.contains("feature/x"), "候補に出るべき\n{out}");
    assert!(
        out.contains("削除済み"),
        "リモート削除済みと表示されるべき\n{out}"
    );
}

#[test]
fn merged_branch_shows_alive_when_remote_present() {
    let (_root, work) = setup();

    // feature を作り push（upstream 設定）→ main にマージ。リモートは残したまま。
    git(&work, &["checkout", "-q", "-b", "feature/y"]);
    commit_file(&work, "c.txt", "c", "feature work");
    git(&work, &["push", "-q", "-u", "origin", "feature/y"]);
    git(&work, &["checkout", "-q", "main"]);
    git(
        &work,
        &["merge", "-q", "--no-ff", "-m", "merge", "feature/y"],
    );

    let assert = Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(&work)
        .arg("-d")
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(out.contains("feature/y"), "候補に出るべき\n{out}");
    assert!(
        out.contains("存在"),
        "リモートに残存と表示されるべき\n{out}"
    );
}
