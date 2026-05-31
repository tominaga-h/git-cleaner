//! 統合テスト: `git-cleaner init` の冪等テンプレート追記。
//!
//! HOME を tempdir に向けて2回実行し、グローバル/ローカル両方の config に
//! テンプレートが入ること、2回目以降はバイト一致（冪等）であること、
//! コメント行が保持されることを検証する。

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

/// HOME=home_dir を設定して init を実行する。
fn run_init(workdir: &Path, home: &Path) {
    Command::cargo_bin("git-cleaner")
        .unwrap()
        .current_dir(workdir)
        .env("HOME", home)
        .arg("init")
        .assert()
        .success();
}

#[test]
fn init_is_idempotent_and_preserves_comments() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let work = root.path().join("work");
    std::fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);

    let global_config = home.join(".gitconfig");
    let local_config = work.join(".git").join("config");

    // 1回目: 両方にテンプレートが追記される。
    run_init(&work, &home);
    let global_after_1 = std::fs::read_to_string(&global_config).unwrap();
    let local_after_1 = std::fs::read_to_string(&local_config).unwrap();

    assert!(
        global_after_1.contains("[cleaner]"),
        "グローバルに [cleaner] が入るべき"
    );
    assert!(
        global_after_1.contains("# PC全体で共通の保護ブランチ"),
        "グローバルのコメントが保持されるべき"
    );
    assert!(
        local_after_1.contains("[cleaner]"),
        "ローカルに [cleaner] が入るべき"
    );
    assert!(
        local_after_1.contains("# targets = develop"),
        "ローカルのコメント行が保持されるべき"
    );

    // 2回目: no-op（バイト一致）。
    run_init(&work, &home);
    let global_after_2 = std::fs::read_to_string(&global_config).unwrap();
    let local_after_2 = std::fs::read_to_string(&local_config).unwrap();

    assert_eq!(
        global_after_1, global_after_2,
        "2回目のグローバルはバイト一致すべき"
    );
    assert_eq!(
        local_after_1, local_after_2,
        "2回目のローカルはバイト一致すべき"
    );
}
