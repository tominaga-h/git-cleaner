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

/// main にマージ済みの feature/0..n を持つ repo を作る（カレントは main）。
fn repo_with_n_merged_features(n: usize) -> TempDir {
    let repo = init_repo();
    let p = repo.path();
    commit_file(p, "a.txt", "a", "init");
    for i in 0..n {
        let branch = format!("feature/{i:02}");
        git(p, &["checkout", "-q", "-b", &branch]);
        commit_file(p, &format!("f{i}.txt"), "x", &format!("work {i}"));
        git(p, &["checkout", "-q", "main"]);
        git(
            p,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("merge {i}"),
                &branch,
            ],
        );
    }
    repo
}

#[test]
fn limit_truncates_candidates_in_dry_run() {
    let repo = repo_with_n_merged_features(5);
    let p = repo.path();

    let assert = Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(p)
        .args(["-d", "-t", "main", "-l", "2"])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // 先頭2件のみ [1/2] [2/2] が出て、[3/...] は出ない。
    assert!(out.contains("[1/2]"), "1件目が出るべき\n{out}");
    assert!(out.contains("[2/2]"), "2件目が出るべき\n{out}");
    assert!(!out.contains("[3/"), "3件目以降は出ないべき\n{out}");
    assert!(
        out.contains("先頭 2 件に絞り込みました"),
        "絞り込み通知\n{out}"
    );
}

#[test]
fn limit_only_deletes_up_to_n_branches() {
    let repo = repo_with_n_merged_features(5);
    let p = repo.path();

    // 全件に y を流しても、--limit 2 なので 2 件しか対象にならない。
    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(p)
        .args(["-t", "main", "-l", "2"])
        .write_stdin("y\ny\ny\ny\ny\n")
        .assert()
        .success();

    // 残存ブランチ数を数える（main + 未削除 feature）。
    let count = (0..5)
        .filter(|i| branch_exists(p, &format!("feature/{i:02}")))
        .count();
    assert_eq!(
        count, 3,
        "5件中2件だけ削除され、3件残るべき（実際の残存: {count}）"
    );
}
