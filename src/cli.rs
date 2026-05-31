//! CLI 定義（フェーズ1・サブコマンドなし方針）。
//!
//! `git-cleaner [OPTIONS]` がメイン動作（マージ済みブランチ掃除）で、
//! `init` のみを別動作（`Command::Init`）として扱う。トップレベルに
//! `-d/--dry-run` と `-t/--target` を持たせ、`command` が `None` の場合は
//! 掃除動作を実行する。将来 `stash`/`file`/`tag` を `Command` に追加できる。

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "git-cleaner",
    about = "Interactively clean up local git branches already merged into base branches.",
    version
)]
pub struct Cli {
    /// 実際の削除は行わず、削除対象の一覧と詳細のみ表示する。
    #[arg(short = 'd', long = "dry-run", global = true)]
    pub dry_run: bool,

    /// Git Config の targets を一時的に上書きするマージ先ブランチ。
    #[arg(short = 't', long = "target", value_name = "BRANCH", global = true)]
    pub target: Option<String>,

    /// 削除対象を抽出後の先頭から最大 N 件に絞る（大量のブランチがある場合に便利）。
    #[arg(short = 'l', long = "limit", value_name = "N", global = true)]
    pub limit: Option<usize>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// `[cleaner]` テンプレートをグローバル/ローカルの git config に追記する。
    Init,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_means_branch_cleanup() {
        let cli = Cli::try_parse_from(["git-cleaner"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.dry_run);
        assert_eq!(cli.target, None);
    }

    #[test]
    fn dry_run_short_and_long_parse() {
        let short = Cli::try_parse_from(["git-cleaner", "-d"]).unwrap();
        assert!(short.dry_run);

        let long = Cli::try_parse_from(["git-cleaner", "--dry-run"]).unwrap();
        assert!(long.dry_run);
    }

    #[test]
    fn target_short_and_long_parse() {
        let short = Cli::try_parse_from(["git-cleaner", "-t", "main"]).unwrap();
        assert_eq!(short.target.as_deref(), Some("main"));

        let long = Cli::try_parse_from(["git-cleaner", "--target", "develop"]).unwrap();
        assert_eq!(long.target.as_deref(), Some("develop"));
    }

    #[test]
    fn dry_run_and_target_combine() {
        let cli = Cli::try_parse_from(["git-cleaner", "-d", "-t", "main"]).unwrap();
        assert!(cli.dry_run);
        assert_eq!(cli.target.as_deref(), Some("main"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn init_subcommand_is_recognized() {
        let cli = Cli::try_parse_from(["git-cleaner", "init"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Init)));
    }

    #[test]
    fn limit_short_and_long_parse() {
        let short = Cli::try_parse_from(["git-cleaner", "-l", "5"]).unwrap();
        assert_eq!(short.limit, Some(5));

        let long = Cli::try_parse_from(["git-cleaner", "--limit", "10"]).unwrap();
        assert_eq!(long.limit, Some(10));

        let none = Cli::try_parse_from(["git-cleaner"]).unwrap();
        assert_eq!(none.limit, None);
    }
}
