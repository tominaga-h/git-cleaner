//! `init` 動作: `[cleaner]` テンプレートを git config に冪等追記する。
//!
//! Task 0 ではスタブのみ。Task 5 でテンプレート追記を実装する。

use anyhow::Result;

/// グローバル/ローカルの git config に `[cleaner]` テンプレートを追記する。
pub fn run() -> Result<()> {
    // TODO(Task 5): ~/.gitconfig と .git/config に未存在時のみ追記。
    println!("[stub] init");
    Ok(())
}
