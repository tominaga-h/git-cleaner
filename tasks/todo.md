# git-cleaner 実装 TODO（フェーズ1 / MVP）

> 詳細は [`docs/IMPLEMENTATION_PLAN.md`](../docs/IMPLEMENTATION_PLAN.md) を参照。
> **各タスクはテストを実装し、`make check`（fmt-check + clippy -D warnings + test）がパスしてから完了とする。**

依存順: `0 → 1 → {2 → 4, 3}`、`5` は `0` 後に並行可、`6` は最後。

---

## [x] Task 0 — プロジェクト雛形 + CLI パース + make check 基盤 ✅

- [x] `Cargo.toml`（clap/git2/chrono/anyhow/dirs + dev-deps）
- [x] `Makefile` / `.gitignore` / `githooks/pre-push.sh` は既存のものを利用（`make check` = fmt-check + cargo check -Dwarnings + clippy + test）
- [x] `src/main.rs`（パース→ディスパッチ→exit code）
- [x] `src/cli.rs`（`Cli { dry_run, target, command: Option<Command> }`, `Command::Init`）
- [x] スタブ `src/cleaner.rs` / `src/init.rs`
- [x] **テスト**: cli パース（無引数→掃除、`-d`/`-t X`、`init` 認識）— 5件 pass
- **Acceptance**: 無引数→掃除スタブ／`-d`・`-t X` パース／`init`→initスタブ／`--help` ✅
- **Verify**: `make check` パス + 手動スモーク（全パス確認済み）✅

## [x] Task 1 — repo オープン + merged 判定（dry-run コア）✅

- [x] `src/git.rs`（open / local_branches / resolve_target_tip / is_merged_into / current_branch）
- [x] `src/cleaner.rs`（`find_candidates` + dry-run 表示。`MergedBranch` 中間表現で repo 非依存に）
- [x] `src/ui.rs`（render_candidate / relative_time）
- [x] **テスト(単体)**: `find_candidates`（merged/current/protect 除外）, `relative_time`, `render_candidate` — 計9件
- [x] **テスト(統合)**: `tests/branch_detection.rs`（main+feature マージ→`-d -t main` で feature のみ表示）
- **Acceptance**: merged のみ列挙／unmerged は出ない／カレント常に除外 ✅
- **Verify**: `make check` パス（15 tests 全通過）✅
- 備考: フェーズ1暫定で `-t` 必須（config 読込は Task 2 で配線）。protect 引数は `find_candidates` に用意済みで Task 2 で結線。

## [x] Task 2 — config 読込 + 結合（global+local union）✅

- [x] `src/config.rs`（`load` / `parse_csv`。union は multivar 全エントリを `parse_csv` でまとめて実現）
- [x] `src/cleaner.rs` へ配線（`--target` override、`cleaner.protect` + カレント常時保護）
- [x] **テスト(単体)**: `parse_csv`（trim/空/dedup）, union（global+local multivar）— 計4件
- [x] **テスト(統合)**: `tests/config_filtering.rs`（config targets 読込・protect 除外・`--target` 上書き）
- **Acceptance**: targets/protect 反映／protect は merged でも除外／`--target` 優先 ✅
- **Verify**: `make check` パス（24 tests 全通過）✅
- 備考: テンプレート定数・`has_cleaner_section` は Task 5 で追加（dead_code 回避のため前倒ししない）。

## [x] Task 3 — リモート生存状態 + fetch --prune ✅

- [x] `src/git.rs`（`fetch_prune` / `remote_branch_alive` → Alive/Deleted/Unknown）
- [x] `src/cleaner.rs`・`src/ui.rs` へ配線（fetch を分析前に実行→失敗時 warn して継続）
- [x] **テスト(単体)**: `remote_branch_alive`（upstream 有=Alive / 無=Unknown / 不在ブランチ=Unknown）— 3件
- [x] **テスト(統合)**: `tests/remote_state.rs`（bare origin で push→削除→prune→「削除済み」/「存在」）— 2件
- **Acceptance**: pruned→削除済み／残存→存在／追跡なし→不明／fetch 失敗は警告して継続 ✅
- **Verify**: `make check` パス（27 tests 全通過）✅
- 備考: Deleted/Unknown 判別は `branch_upstream_name`（config 上の upstream 名の有無）で行う。

## [x] Task 4 — 対話削除（y/N/skip + git branch -d）✅

- [x] `src/ui.rs`（`prompt` / `Decision`、reader/writer 注入、EOF→Skip）
- [x] `src/git.rs`（`delete_branch` シェルアウト、stderr surface）
- [x] `src/cleaner.rs`（1件ずつ提示→prompt→Delete で削除、`--dry-run` は一覧のみ）
- [x] **テスト(単体)**: `prompt`（y/yes/Y→Delete、n/空/skip/s→Skip、不正→再プロンプト、EOF→Skip）— 4件
- [x] **テスト(統合)**: `tests/interactive_delete.rs`（`y`で消失/`n`で残存/`--dry-run`で削除なし）— 3件
- **Acceptance**: `y`削除／`n`スキップ／`--dry-run`削除しない ✅
- **Verify**: `make check` パス（34 tests 全通過）✅

## [x] Task 5 — init コマンド（冪等テンプレート追記）✅

- [x] `src/config.rs`（`GLOBAL_TEMPLATE` / `LOCAL_TEMPLATE` / `has_cleaner_section`）
- [x] `src/init.rs`（`~/.gitconfig` と `repo.path()/config` に未存在時のみ raw 追記、`AppendResult`）
- [x] **テスト(単体)**: `has_cleaner_section`（ヘッダ/subsection/近似名）, `append_template_if_absent`（新規/冪等/既存保持）— 計7件
- [x] **テスト(統合)**: `tests/init_idempotent.rs`（HOME=tempdir で2回実行→バイト一致、コメント保存）
- **Acceptance**: 初回追記／2回目 no-op／既存不変／コメント保存 ✅
- **Verify**: `make check` パス（41 tests 全通過）+ 手動で実バイト確認（git が `cleaner.protect` を読めることも確認）✅

## [x] Task 6 — 仕上げ ✅

- [x] エラーメッセージ（非 repo はクリーンな日本語に整備 / targets 未設定 / fetch 失敗は warn 継続 / 候補ゼロ）
- [x] ヘルプ文言・exit code（非 repo・targets 未設定=1 / 候補ゼロ=0）
- [x] `README.md`
- [x] **テスト(統合)**: `tests/error_paths.rs`（非 repo→失敗 / targets 未設定→失敗+ヒント / 候補ゼロ→成功+メッセージ）— 3件
- **Acceptance**: 各エラーパスで適切なメッセージ／fetch 失敗は継続／候補ゼロ明示 ✅
- **Verify**: `make check` パス（44 tests 全通過）+ 手動スモーク（非 repo / --help / --version）✅

---

## チェックポイント（人間レビュー）

- [x] Task 0 後: clap 構成の確認
- [x] Task 1 後: 実マージで merged 判定の正しさ・`find_candidates` の seam 確認
- [x] Task 3 後: リモート生存定義・fetch 失敗 degrade 確認
- [x] Task 5 後: 書き込む実バイト・冪等性・コメント保存の確認
- [x] Task 6 / リリース前: full dry-run → 使い捨て repo で実削除
