# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 概要

`git-cleaner` は、ベースブランチ（`develop` / `main` 等）にマージ済みのローカルブランチを検出し、対話的に削除する Rust 製 CLI。設定は独自ファイルを持たず、Git 標準の config の `[cleaner]` セクションに統合する。ユーザー向けの文言は日本語。

## コマンド

```bash
make check                                   # ローカルの正規ゲート: githooks/pre-push.sh --no-hook --fix を実行
                                             # = cargo fmt（自動修正）+ cargo check + clippy + test、すべて -Dwarnings
cargo build                                  # デバッグビルド
cargo build --release                        # target/release/git-cleaner
cargo test --all-targets                     # 全テスト（ユニット + 統合）
cargo test --lib <test_name>                 # ユニットテスト単体（inline #[cfg(test)] モジュール）
cargo test --test <file> <test_name>         # 統合テスト単体（例: --test branch_detection）
cargo clippy --all-targets -- -D warnings    # lint（警告はエラー扱い）
cargo fmt --all -- --check                   # フォーマットチェック（修正なし）
make install-hooks                           # 上記チェックを実行する pre-push フックを導入
```

push 前に `make check` を実行すること。CI と完全に一致し、フォーマットも自動修正する。CI（`.github/workflows/ci.yml`）は `check` / `test` / `fmt` / `clippy` を別ジョブとして `RUSTFLAGS=-Dwarnings` で実行する。`.github/workflows/release.yml` は `v*` タグでクロスプラットフォームのバイナリをビルドする。

## アーキテクチャ

エントリ: `src/main.rs` が引数を解析（`cli.rs`）し、`init::run()`（`init` サブコマンド）か `cleaner::run(dry_run, target, limit)`（デフォルト）へ振り分ける。

```
main.rs ──> cli.rs                    (clap による引数解析)
        ──> init.rs ──> config.rs     ([cleaner] テンプレートを冪等に追記)
        └─> cleaner.rs                (コア処理のオーケストレーション)
              ├─> git.rs              (git2 による読み取り + git シェルアウトによる変更)
              ├─> config.rs           (cleaner.targets / cleaner.protect の読み込み)
              └─> ui.rs               (候補の表示 + 対話プロンプト)
```

`cleaner.rs` の処理フロー: リポジトリを開く → 裏で `git fetch --prune` を起動 → `targets`/`protect` 設定をロード（global + local の union、カレントブランチは常に保護）→ 各ローカルブランチについて各ターゲットに対するマージ状態を判定 → 保護ブランチ・カレントを除外して `Candidate` を構築 → dry-run 表示 または 候補ごとの対話削除。

### 設計上の要点（複数ファイルを読む必要があるもの）

- **git アクセスのハイブリッド構成**: 読み取り系の解析（ブランチ列挙、マージ判定、履歴走査）はすべて `git2`。ただし**変更系（`fetch --prune` / `branch -d` / `branch -D`）は `git` バイナリへシェルアウト**し、ユーザー既存の認証・認証情報をそのまま使う。`git.rs` 参照。

- **マージ判定がコアで非自明**（`git.rs`、`MergeStatus` まわり 135〜170 行付近）。ターゲット履歴の first-parent 走査で次を区別する:
  - `Fully`（完全マージ）— ブランチの tip がターゲットの祖先に含まれる。
  - `Partially`（部分マージ）— マージコミットで一度取り込まれたが、その後ブランチ側にターゲット未取り込みのコミットを追加したもの。`⚠` 警告付きで表示し、`git branch -d` 自身の安全機構が削除を拒否する。
  - `NotMerged` — 除外。
  検出対象は**通常マージ（マージコミットが残る形式）のみ**。Squash / Rebase マージ（ハッシュが変わる）は設計上対象外。

- **未 push コミットの扱い**: ターゲットにはマージ済みでも `origin/<name>` へ未 push のローカルコミットがあるブランチは `⚠ 未 push` 警告を表示し、プロンプトが `(y/N/skip/force)` になる。`force`/`f` で `git branch -D` を実行。`y` は `git branch -d` を試み、git が拒否すればそのエラーを表示する。

- **設定は git-config ネイティブ**: `config.rs` は `cleaner.targets` / `cleaner.protect` を global（`~/.gitconfig`）と local（`.git/config`）スコープ横断の multivar union として読む。`init.rs` は `[cleaner]` テンプレートをコメント保持のため git2 のセッターではなく生テキストで追記し、冪等（既に `[cleaner]` セクションがあれば何もしない）。

### 主要な型

- `Candidate`（`cleaner.rs`）— マージ状態・リモート状態・未 push フラグを持つ削除候補。
- `MergedBranch`（`cleaner.rs`）— git2 非依存の中間結果。フィルタロジックをユニットテストできるよう素のデータにしてある。
- `LocalBranch`（`git.rs`）— 名前、tip OID、コミット時刻、サマリ。
- `MergeStatus` / `RemoteState` / `MergeInfo`（`git.rs`）— マージ分類、`origin/<name>` の生存状態（Alive/Deleted/Unknown）、取り込んだマージコミットの短縮ハッシュ + 日時。

## テスト

inline `#[cfg(test)]` モジュール（`cli.rs`, `config.rs`, `git.rs`, `init.rs`, `ui.rs`）と、`tests/` 配下の統合テスト（`branch_detection.rs`, `config_filtering.rs`, `error_paths.rs`, `init_idempotent.rs`, `interactive_delete.rs`, `remote_state.rs`）の両方がある。統合テストは `tempfile` + `git` CLI で実リポジトリを構築し、`assert_cmd` でバイナリを駆動する。対話・I/O ロジックは I/O を注入する設計で TTY なしでもプロンプトをテストできる。`ui.rs` / `cleaner.rs` を変更する際はこの境界を保つこと。
