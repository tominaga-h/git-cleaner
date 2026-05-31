# Changelog

このプロジェクトの主な変更点を記録します。

フォーマットは [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に準拠し、
バージョニングは [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [1.0.0] - 2026-05-31

### Added

- ベースブランチ（`develop` / `main` 等）にマージ済みのローカルブランチを検出し、対話的に削除する機能。
- `git-cleaner init` による `[cleaner]` 設定テンプレートの冪等な追記。
- Git config（`cleaner.targets` / `cleaner.protect`）ネイティブな設定読み込み（global + local の union）。カレントブランチは常に保護。
- `-d` / `--dry-run`: 実削除せず削除対象の一覧と詳細を表示。
- `-t` / `--target`: `cleaner.targets` の一時上書き。
- `-l` / `--limit`: 削除対象を先頭 N 件に絞る。
- マージ日時・マージコミット短縮ハッシュ・最終コミット日時（相対時間）・コミットメッセージ・リモートブランチ生存状態の表示。
- 部分マージブランチの `⚠` 警告表示。
- 未 push コミットを持つブランチの `⚠ 未 push` 警告と `(y/N/skip/force)` プロンプト（`force` / `f` で `git branch -D`）。
- GitHub Actions による CI（`check` / `test` / `fmt` / `clippy`）とクロスプラットフォームのリリースビルド。

[1.0.0]: https://github.com/tominaga-h/git-cleaner/releases/tag/v1.0.0
