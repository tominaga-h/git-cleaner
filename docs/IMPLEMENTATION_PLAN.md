# git-cleaner 実装計画（フェーズ1 / MVP）

## 1. 背景

`docs/OVERVIEW.md` の要件定義に基づき、Rust 製 CLI ツール **git-cleaner** を新規実装する。
フェーズ1（MVP）のゴールは「**ベースブランチ（develop/main 等）にマージ済みのローカルブランチを安全に検出し、ユーザー確認のもと対話的に削除する**」こと。独自設定ファイルは作らず、Git 標準の config（`[cleaner]` セクション）に統合する。

### 確定した方針

- **Git ライブラリ: `git2` + 部分シェルアウト**
  検査系（config 読込・merged 判定・ブランチ列挙・コミット日時）は `git2` で実装。`git fetch --prune` と `git branch -d` のみ `git` バイナリへシェルアウト（認証問題の回避・git の安全削除セマンティクスの踏襲）。
- **フェーズ1ではサブコマンド形式にしない → オプションはトップレベル**
  `git-cleaner -d -t <branch>` のようにトップレベルでフラグを受ける。`branch` サブコマンドは作らない。`init` のみ別動作として扱う。将来のサブコマンド拡張余地は残す設計とする。
- **各タスクでテストコードを実装する**
  各 Task の成果物に、そのスライスをカバーするテスト（単体/統合）を必ず含める。テストなしで完了とはしない。
- **検証は毎回 `make check`**
  `make check` = `cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test`。各 Task の完了条件は「`make check` がパスすること」。

---

## 2. CLI 設計（フェーズ1・サブコマンドなし）

```
git-cleaner [OPTIONS]      # マージ済みブランチ掃除（メイン動作）
git-cleaner init           # [cleaner] テンプレートを git config に追記
```

トップレベルオプション:

- `-d`, `--dry-run` … 削除せず対象一覧・詳細表示のみ
- `-t <BRANCH>`, `--target <BRANCH>` … config の targets を一時上書き

clap 構成: `Cli { dry_run, target, command: Option<Command> }` とし、`Command::Init` のみ enum に持たせる。`command` が `None` ならブランチ掃除動作。将来 `stash`/`file`/`tag` を enum に追加すれば拡張可能。`init` 受信時はトップレベルフラグを無視する。

---

## 3. モジュール構成（src/）

フラットに小さく保つ。過度な抽象化はしない。

```
src/
  main.rs     # エントリ: CLI パース→ディスパッチ→exit code。エラーは stderr。
  cli.rs      # clap 定義: Cli, Command(Init のみ)。no-subcommand=掃除動作。
  config.rs   # cleaner.* のマージ読込、CSV パース/結合、init テンプレート、冪等チェック。
  git.rs      # git2 ファサード + シェルアウト。repo操作の唯一の窓口。
  cleaner.rs  # 掃除動作のオーケストレーション + find_candidates フィルタ（テストの要）。
  ui.rs       # 表示整形 + 対話プロンプト(y/N/skip)。reader/writer 注入でテスタブル。
  init.rs     # init 動作: テンプレートの冪等追記。
```

### 主要 API

- **cli.rs**: `struct Cli { dry_run, target: Option<String>, command: Option<Command> }`, `enum Command { Init }`
- **config.rs**:
  - `struct CleanerConfig { targets: Vec<String>, protect: Vec<String> }`
  - `fn load(repo) -> Result<CleanerConfig>`（global+local の union、multivar 対応）
  - `fn parse_csv(&[String]) -> Vec<String>`（純粋・単体テスト）
  - `fn merge(global, local) -> Vec<String>`（union+dedup・単体テスト）
  - `const GLOBAL_TEMPLATE / LOCAL_TEMPLATE`（§4.2/§4.3 の INI ブロック）
  - `fn has_cleaner_section(&str) -> bool`（冪等チェック・単体テスト）
- **git.rs**:
  - `fn open() -> Result<Repository>`（`Repository::discover(".")`）
  - `fn current_branch(repo) -> Result<Option<String>>`
  - `fn local_branches(repo) -> Result<Vec<LocalBranch>>`（name, tip:Oid, last_commit_time）
  - `fn resolve_target_tip(repo, name) -> Result<Option<Oid>>`
  - `fn is_merged_into(repo, branch_tip, target_tip) -> Result<bool>`（`graph_descendant_of`）
  - `fn remote_branch_alive(repo, name) -> Result<RemoteState>`（Alive/Deleted/Unknown）
  - `fn fetch_prune() -> Result<()>`（シェルアウト）
  - `fn delete_branch(name) -> Result<()>`（シェルアウト、git の stderr を surface）
- **cleaner.rs**:
  - `fn run(dry_run: bool, target: Option<String>) -> Result<()>`（§5 のフロー）
  - `struct Candidate { name, matched_target, last_commit, remote_state }`
  - `fn find_candidates(branches, cfg, target_override, current) -> Vec<Candidate>`（最重要のテスト seam。解決済み入力を受け、実 repo 不要）
- **ui.rs**:
  - `fn render_candidate(idx, total, &Candidate) -> String`（純粋整形）
  - `fn relative_time(then, now) -> String`（"今日"/"N日前"・純粋）
  - `enum Decision { Delete, Skip }`
  - `fn prompt(reader: &mut impl BufRead, writer: &mut impl Write, &Candidate) -> Result<Decision>`
- **init.rs**: `fn run() -> Result<()>`（`~/.gitconfig` と `repo.path()/config` に冪等追記）

---

## 4. Cargo.toml

```toml
[package]
name = "git-cleaner"
version = "0.1.0"
edition = "2021"
description = "Interactively clean up local git branches already merged into base branches."
license = "MIT OR Apache-2.0"

[[bin]]
name = "git-cleaner"
path = "src/main.rs"

[dependencies]
clap = { version = "4.6", features = ["derive"] }
git2 = "0.21"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
anyhow = "1"
dirs = "5"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

## 5. Makefile（全タスクの検証ゲート）

```makefile
.PHONY: check fmt fmt-check clippy test
check: fmt-check clippy test
fmt:
	cargo fmt
fmt-check:
	cargo fmt --check
clippy:
	cargo clippy -- -D warnings
test:
	cargo test
```

---

## 6. 垂直スライス（実装タスク）

各タスクは「単独でビルド・検証できる1本の完結したパス」。横方向レイヤーではなく縦方向に切る。
**各タスクはテストコードの実装を含み、完了条件は `make check`（fmt + clippy + test）がパスすること。**

### Task 0 — プロジェクト雛形 + CLI パース + make check 基盤（薄いE2E）

- **Goal**: `cargo run` がビルドでき、`git-cleaner`（フラグ付き）と `git-cleaner init` がパースされ各スタブに到達。`make check` が回る基盤を整える。
- **Files**: `Cargo.toml`, `Makefile`, `src/main.rs`, `src/cli.rs`, スタブ `src/cleaner.rs`, スタブ `src/init.rs`, `.gitignore`
- **テスト**: `cli` のパーステスト（無引数→掃除動作、`-d`/`-t X` のパース、`init` 認識）を `#[cfg(test)]` で追加。
- **Acceptance**: 無引数→掃除スタブ。`-d`/`--dry-run`・`-t X`/`--target X` がパース。`init`→init スタブ。`--help` 表示。
- **Verify**: `make check` がパス。加えて手動スモーク `cargo run -- -d -t main` / `cargo run -- init` / `cargo run -- --help`。

### Task 1 — repo オープン + merged 判定（dry-run コア、fetch/config なし）

- **Goal**: カレント repo に対し、`--target` 指定（or 暫定デフォルト）にマージ済みのローカルブランチを列挙し、カレントを除外して name+target+最終コミット日時を表示。
- **Files**: `src/git.rs`, `src/cleaner.rs`（find_candidates+dry-run表示）, `src/ui.rs`（render_candidate/relative_time）
- **テスト**:
  - 単体: `find_candidates`（merged/current 除外）, `relative_time`（"今日"/"N日前" 境界）, `render_candidate`（整形）
  - 統合 `tests/branch_detection.rs`: temp repo helper で main+feature を作りマージ→`assert_cmd` で `-d -t main` 実行、feature が出て main が出ないことを assert
- **Acceptance**: merged ブランチのみ列挙、unmerged は出ない、カレントは常に除外。
- **Verify**: `make check` がパス。

### Task 2 — config 読込 + 結合（global+local union、カレント常時保護）

- **Goal**: ハードコード target を `cleaner.targets` のマージ値に置換。`cleaner.protect` を適用。`--target` は targets を上書き。カレントブランチは常に保護。
- **Files**: `src/config.rs`（load/parse_csv/merge）, `src/cleaner.rs` に配線
- **テスト**:
  - 単体: `parse_csv`（trim/空要素）, `merge`（union+dedup。例: `{main,master}`+`{staging}`→`{main,master,staging}`）
  - 統合 `tests/config_filtering.rs`: temp repo で `git config cleaner.targets/protect` 設定→protect 指定ブランチが merged でも除外、`--target` が config に勝つことを assert
- **Acceptance**: `cleaner.targets`/`protect` が反映。protect は merged でも除外。`--target` が config に勝つ。
- **Verify**: `make check` がパス。

### Task 3 — リモート生存状態 + fetch --prune

- **Goal**: `git fetch --prune` を（分析開始前に spawn し分析直前に await して）実行し、各候補のリモート生存状態を表示。
- **Files**: `src/git.rs`（fetch_prune/remote_branch_alive）, `src/cleaner.rs`・`src/ui.rs` に配線
- **テスト**:
  - 統合 `tests/remote_state.rs`: temp の bare repo を origin として作り push→リモート側削除→fetch --prune→「削除済み」/「存在」/「不明」表示を assert
  - `remote_branch_alive` を fixture repo（remote ref 有/無）で単体テスト
- **Acceptance**: pruned→「削除済み」、残存→「存在」、追跡なし→「不明」。fetch 失敗時は警告して継続。
- **Verify**: `make check` がパス。実リモートへの live fetch のみ手動確認。

### Task 4 — 対話削除（y/N/skip ループ + git branch -d）

- **Goal**: §6 のプロンプトループ。`y`→`git branch -d`、`n`/skip→次へ、結果を1件ずつ表示。`--dry-run` 時は削除しない。
- **Files**: `src/ui.rs`（prompt/Decision）, `src/git.rs`（delete_branch）, `src/cleaner.rs`（ループ駆動）
- **テスト**:
  - 単体: `prompt` を fake reader（"y\n"/"n\n"/空=既定N/"skip"/不正→再プロンプト）で検証
  - 統合 `tests/interactive_delete.rs`: stdin に `y\n` を流しブランチ消失、`n\n` で残存、`--dry-run` は削除なしを assert
- **Acceptance**: `y` で削除、`n` でスキップ、`--dry-run` は削除しない。
- **Verify**: `make check` がパス。対話TTYの体感のみ手動確認。

### Task 5 — init コマンド（冪等テンプレート追記）

- **Goal**: `[cleaner]` テンプレート（コメント付き）を `~/.gitconfig` と `.git/config` にセクション未存在時のみ追記。重複しない。
- **Files**: `src/init.rs`, `src/config.rs` のテンプレート定数
- **テスト**:
  - 単体: `has_cleaner_section`（ヘッダ検出のバリエーション）
  - 統合 `tests/init_idempotent.rs`: `HOME` を tempdir に向け2回実行→2回目以降ファイルがバイト一致、コメント行が保存を assert
- **Acceptance**: 初回は両方追記、2回目は no-op、既存内容は不変、コメント保存。
- **Verify**: `make check` がパス。

### Task 6 — 仕上げ

- **Goal**: エラーメッセージ（非 repo / targets 未設定 / fetch 失敗 / 候補ゼロ）、ヘルプ文言、exit code、README。
- **Files**: 全体 + `README.md`
- **テスト**: 統合 `tests/error_paths.rs`: 非 git repo・`cleaner.targets` 未設定・候補ゼロ の各メッセージ/exit code を assert
- **Acceptance**: 各エラーパスで適切なメッセージ。fetch 失敗は warn して継続。候補ゼロで明示メッセージ。
- **Verify**: `make check` がパス。各エラーパスのスモーク手動確認。

---

## 7. 依存グラフ

```
Task 0 (雛形)
   ├─> Task 1 (merged判定/dry-runコア)
   │      ├─> Task 2 (config 読込+結合)
   │      │       └─> Task 4 (対話削除)
   │      └─> Task 3 (fetch + リモート生存)
   └─> Task 5 (init)  ← Task 0 後すぐ着手可・並行可能
Tasks 1–5 ──> Task 6 (仕上げ)
```

- **0 → 全て**
- **1 → 2,3,4**（Candidate モデルが共有依存）
- **2 → 4**（削除は protect/current/target を尊重）
- **3** は 1 後に 2/4 と並行可（リモート生存は表示専用）
- **5(init)** は 0 後すぐ並行可能

---

## 8. リスク / 注意点

1. **merged 判定**: `repo.graph_descendant_of(target_tip, branch_tip)` を使用（target が branch tip を含む＝通常マージ/FF）。`git branch --merged` のパースはしない。targets の先頭から最初に含む target にマッチさせ、表示用に記録。squash/rebase はフェーズ1対象外（false negative のみ＝安全）。
2. **リモート生存（prune後）**: branch の upstream（`branch.upstream()`）優先で `refs/remotes/<remote>/<name>` を確認。pruned→「削除済み」、残存→「存在」、upstream無し→「不明/未追跡」。
3. **config 結合**: 要件は targets/protect の union。multivar として全エントリを走査→カンマ分割→dedup union。例: global `protect=main,master` + local `protect=staging` → `{main,master,staging}`。
4. **init 冪等**: `set_str` は使わない（コメント付きテンプレートが消える）。テキスト読込→`has_cleaner_section` 判定→未存在時のみ raw ブロック追記。`~/.gitconfig` 不在時は作成。`.git/config` は `repo.path().join("config")` で解決（worktree 対応）。
5. **無引数デフォルト**: `Cli.command: Option<Command>`。`None`→掃除動作。`-d`/`-t` はトップレベル。`init` 時はフラグ無視。
6. **プロンプトのテスタビリティ**: `prompt(reader, writer, ...)` に IO を注入。本番は `stdin().lock()`/`stdout().lock()`、テストは `&b"y\n"[..]` と `Vec<u8>`。
7. **fetch の遅延/失敗**: ローカル分析前に child を spawn→リモート生存計算直前に wait。失敗時は warn して継続（生存状態は「不明」）。

---

## 9. テスト戦略

- **純粋単体テスト**: `parse_csv`, `merge`, `has_cleaner_section`, `relative_time`, `render_candidate`, `prompt`(スクリプト reader), `find_candidates`(構成済み入力でフィルタ検証＝最重要 seam)
- **統合テスト（tempfile + assert_cmd）**: merged/unmerged 分類、protect/current 除外、`--dry-run` は削除なし、対話削除、init 冪等、リモート生存
- **手動のみ**: 実リモートへの live fetch（認証面）、対話TTYの体感/日本語表示、実 `~/.gitconfig` 追記
- **検証ゲート（毎タスク）**: `make check`（fmt-check + clippy -D warnings + test）。グリーンになるまで完了としない。

---

## 10. チェックポイント（人間レビュー）

- **Task 0 後**: clap 構成（トップレベルフラグ + `init` の扱い）の確認。
- **Task 1 後**: 実 repo の実マージで merged 判定の正しさを確認。`find_candidates` の seam 境界をレビュー。
- **Task 3 後**: 「リモート生存」定義と fetch 失敗時の degrade を確認。
- **Task 5 後**: `~/.gitconfig`・`.git/config` に書き込む実バイトを確認（唯一グローバル config を変更する箇所）。冪等性とコメント保存を検証。
- **Task 6 / リリース前**: full dry-run → 使い捨て repo で実削除を手動確認。
