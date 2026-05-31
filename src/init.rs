//! `init` 動作: `[cleaner]` テンプレートを git config に冪等追記する。
//!
//! - グローバル（`~/.gitconfig`）: 保護ブランチのデフォルト（`GLOBAL_TEMPLATE`）
//! - ローカル（`.git/config`）: プロジェクト固有のテンプレート（`LOCAL_TEMPLATE`）
//!
//! `set_str` は使わず raw テキストを追記することで、テンプレート内のコメント行を
//! 保持する。`[cleaner]` セクションが既に存在する場合は何もしない（冪等）。

use crate::config::{self, GLOBAL_TEMPLATE, LOCAL_TEMPLATE};
use crate::git;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// テンプレート追記の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendResult {
    /// 新規にテンプレートを追記した。
    Appended,
    /// 既に `[cleaner]` セクションがあるため何もしなかった。
    Skipped,
}

/// グローバル/ローカルの git config に `[cleaner]` テンプレートを追記する。
pub fn run() -> Result<()> {
    // ローカル config パスは repo を開いて解決（worktree でも正しく効く）。
    let repo = git::open()?;
    let local_path = repo.path().join("config");

    // グローバル config パス（~/.gitconfig）。
    let global_path = global_config_path().context("ホームディレクトリを特定できません")?;

    match append_template_if_absent(&global_path, GLOBAL_TEMPLATE)? {
        AppendResult::Appended => {
            println!(
                "グローバル設定にテンプレートを追記しました: {}",
                global_path.display()
            )
        }
        AppendResult::Skipped => println!(
            "グローバル設定には既に [cleaner] があります（スキップ）: {}",
            global_path.display()
        ),
    }

    match append_template_if_absent(&local_path, LOCAL_TEMPLATE)? {
        AppendResult::Appended => {
            println!(
                "ローカル設定にテンプレートを追記しました: {}",
                local_path.display()
            )
        }
        AppendResult::Skipped => println!(
            "ローカル設定には既に [cleaner] があります（スキップ）: {}",
            local_path.display()
        ),
    }

    Ok(())
}

/// `~/.gitconfig` のパスを返す。
fn global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".gitconfig"))
}

/// `[cleaner]` セクションが無ければ `template` を末尾に追記する（冪等）。
///
/// ファイルが存在しなければ新規作成して追記する。既存内容の末尾が改行で
/// 終わっていない場合は改行を1つ挟んでから追記する。
fn append_template_if_absent(path: &Path, template: &str) -> Result<AppendResult> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(e).with_context(|| format!("{} の読み込みに失敗しました", path.display()))
        }
    };

    if config::has_cleaner_section(&existing) {
        return Ok(AppendResult::Skipped);
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(template);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("{} の作成に失敗しました", parent.display()))?;
        }
    }
    std::fs::write(path, next)
        .with_context(|| format!("{} の書き込みに失敗しました", path.display()))?;
    Ok(AppendResult::Appended)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_to_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitconfig");
        let r = append_template_if_absent(&path, GLOBAL_TEMPLATE).unwrap();
        assert_eq!(r, AppendResult::Appended);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(config::has_cleaner_section(&contents));
        // コメント行が保持されている。
        assert!(contents.contains("# PC全体で共通の保護ブランチ"));
    }

    #[test]
    fn is_idempotent_on_second_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");

        let first = append_template_if_absent(&path, LOCAL_TEMPLATE).unwrap();
        assert_eq!(first, AppendResult::Appended);
        let after_first = std::fs::read_to_string(&path).unwrap();

        let second = append_template_if_absent(&path, LOCAL_TEMPLATE).unwrap();
        assert_eq!(second, AppendResult::Skipped);
        let after_second = std::fs::read_to_string(&path).unwrap();

        // 2回目は何も追記しない（バイト一致）。
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn preserves_existing_content_and_adds_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        // 末尾が改行でない既存内容。
        std::fs::write(&path, "[core]\n\teditor = vim").unwrap();

        let r = append_template_if_absent(&path, GLOBAL_TEMPLATE).unwrap();
        assert_eq!(r, AppendResult::Appended);

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[core]"), "既存内容は保持されるべき");
        assert!(config::has_cleaner_section(&contents));
        // [core] 行と [cleaner] が改行で分かれている。
        assert!(contents.contains("editor = vim\n[cleaner]"));
    }
}
