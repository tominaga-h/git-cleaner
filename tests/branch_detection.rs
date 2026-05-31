//! 統合テスト: merged ブランチ検出（dry-run コア）。
//!
//! temp repo を作り、feature を main にマージ→`-d -t main` 実行で
//! feature が候補に出て、main（カレント）は出ないことを検証する。

use assert_cmd::Command;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;

/// temp ディレクトリに git repo を初期化し、最小設定を入れる。
fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    dir
}

/// 指定ディレクトリで git コマンドを実行する（失敗時 panic）。
fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed", args);
}

/// ファイルを作り、メッセージ付きでコミットする。
fn commit_file(dir: &Path, file: &str, content: &str, msg: &str) {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

#[test]
fn merged_branch_is_listed_unmerged_and_current_are_not() {
    let repo = init_repo();
    let p = repo.path();

    // main に初期コミット。
    commit_file(p, "a.txt", "a", "init");

    // feature/merged: main から分岐→コミット→main にマージ（マージコミット）。
    git(p, &["checkout", "-q", "-b", "feature/merged"]);
    commit_file(p, "b.txt", "b", "feature work");
    git(p, &["checkout", "-q", "main"]);
    git(
        p,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge feature",
            "feature/merged",
        ],
    );

    // feature/unmerged: main から分岐→コミット（未マージのまま）。
    git(p, &["checkout", "-q", "-b", "feature/unmerged"]);
    commit_file(p, "c.txt", "c", "unmerged work");

    // カレントは main に戻す。
    git(p, &["checkout", "-q", "main"]);

    let assert = Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(p)
        .args(["-d", "-t", "main"])
        .assert()
        .success();

    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        out.contains("feature/merged"),
        "merged ブランチが候補に出るべき\n{out}"
    );
    assert!(
        !out.contains("feature/unmerged"),
        "未マージブランチは候補に出ないべき\n{out}"
    );
    // カレント(main)は候補に出ない（行頭の候補表示として）。
    assert!(
        !out.contains("ブランチ 'main'"),
        "カレントブランチは候補に出ないべき\n{out}"
    );
    assert!(out.contains("dry-run"), "dry-run 表示があるべき\n{out}");
}
