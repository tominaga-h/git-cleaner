//! `cleaner.*` 設定の読み込みと、global/local 値の結合（union）。
//!
//! 要件: `cleaner.targets` / `cleaner.protect` は global と local の両方の値を
//! 「結合（マージ）」して扱う。git config の multivar として全スコープのエントリ
//! を走査し、カンマ分割→trim→dedup union する。
//!
//! init テンプレート（Task 5 で使用）もここに定数として持つ。

use anyhow::{Context, Result};
use git2::Repository;

/// 結合済みの cleaner 設定。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanerConfig {
    /// マージ先として判定するベースブランチ。
    pub targets: Vec<String>,
    /// 削除対象から除外する保護ブランチ。
    pub protect: Vec<String>,
}

/// repo の config（local+global+system のマージビュー）から cleaner 設定を読む。
pub fn load(repo: &Repository) -> Result<CleanerConfig> {
    let cfg = repo.config().context("git config を開けません")?;
    Ok(CleanerConfig {
        targets: read_multi(&cfg, "cleaner.targets")?,
        protect: read_multi(&cfg, "cleaner.protect")?,
    })
}

/// 指定キーの全 multivar エントリを集め、カンマ分割して union する。
fn read_multi(cfg: &git2::Config, key: &str) -> Result<Vec<String>> {
    let mut raw: Vec<String> = Vec::new();
    // multivar はキー未設定だと NotFound を返すので、その場合は空として扱う。
    match cfg.multivar(key, None) {
        Ok(mut entries) => {
            while let Some(entry) = entries.next() {
                let entry = entry.with_context(|| format!("config '{key}' の読み取りに失敗"))?;
                if let Ok(value) = entry.value() {
                    raw.push(value.to_string());
                }
            }
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("config '{key}' の取得に失敗")),
    }
    Ok(parse_csv(&raw))
}

/// カンマ区切り文字列群を分割・trim し、空要素を除いて順序保持で dedup する。
pub fn parse_csv(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        for item in value.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if !out.iter().any(|existing| existing == item) {
                out.push(item.to_string());
            }
        }
    }
    out
}

/// グローバル設定（`~/.gitconfig`）の初期テンプレート。
pub const GLOBAL_TEMPLATE: &str = "\
[cleaner]
\t# PC全体で共通の保護ブランチ（デフォルト設定）
\tprotect = main,master
";

/// ローカル設定（`.git/config`）の初期テンプレート。
pub const LOCAL_TEMPLATE: &str = "\
[cleaner]
\t# このプロジェクトにおけるマージ先ベースブランチ（カンマ区切り）
\t# targets = develop

\t# このプロジェクト固有で追加したい保護ブランチ（例: staging）
\t# protect = staging
";

/// config テキストに `[cleaner]` セクションが既に存在するか（init の冪等判定）。
pub fn has_cleaner_section(contents: &str) -> bool {
    contents.lines().any(|line| {
        let trimmed = line.trim();
        // セクションヘッダ [cleaner] か [cleaner "..."]（subsection）を検出する。
        let header = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
        match header {
            Some(name) => {
                let name = name.trim();
                name.eq_ignore_ascii_case("cleaner")
                    || name.to_ascii_lowercase().starts_with("cleaner ")
            }
            None => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_csv_trims_and_drops_empty() {
        let input = s(&[" develop , main ", "", " , feature"]);
        assert_eq!(parse_csv(&input), s(&["develop", "main", "feature"]));
    }

    #[test]
    fn parse_csv_dedups_preserving_order() {
        let input = s(&["main,develop", "develop,release"]);
        assert_eq!(parse_csv(&input), s(&["main", "develop", "release"]));
    }

    #[test]
    fn union_of_global_and_local_multivar() {
        // multivar の各スコープ値（global の "main,master" と local の "staging"）を
        // まとめて parse_csv に渡すと union になる。これが load の結合ロジック。
        let global_and_local = s(&["main,master", "staging"]);
        assert_eq!(
            parse_csv(&global_and_local),
            s(&["main", "master", "staging"])
        );
    }

    #[test]
    fn union_dedups_overlap_across_scopes() {
        let global_and_local = s(&["main,master", "master,staging"]);
        assert_eq!(
            parse_csv(&global_and_local),
            s(&["main", "master", "staging"])
        );
    }

    #[test]
    fn has_cleaner_section_detects_header() {
        assert!(has_cleaner_section("[cleaner]\n\tprotect = main"));
        assert!(has_cleaner_section("[core]\n[cleaner]\n"));
        assert!(has_cleaner_section("  [cleaner]  "));
    }

    #[test]
    fn has_cleaner_section_detects_subsection() {
        assert!(has_cleaner_section("[cleaner \"foo\"]\n"));
    }

    #[test]
    fn has_cleaner_section_false_when_absent() {
        assert!(!has_cleaner_section("[core]\n\teditor = vim\n"));
        assert!(!has_cleaner_section(""));
        // 紛らわしい近似名は誤検出しない。
        assert!(!has_cleaner_section("[cleanerx]\n"));
    }

    #[test]
    fn templates_contain_cleaner_section() {
        assert!(has_cleaner_section(GLOBAL_TEMPLATE));
        assert!(has_cleaner_section(LOCAL_TEMPLATE));
    }
}
