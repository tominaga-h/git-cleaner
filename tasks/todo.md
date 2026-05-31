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

## [ ] Task 2 — config 読込 + 結合（global+local union）

- [ ] `src/config.rs`（load / parse_csv / merge）
- [ ] `src/cleaner.rs` へ配線（`--target` override、カレント常時保護）
- [ ] **テスト(単体)**: `parse_csv`（trim/空）, `merge`（union+dedup）
- [ ] **テスト(統合)**: `tests/config_filtering.rs`（protect 除外、`--target` が config に勝つ）
- **Acceptance**: targets/protect 反映／protect は merged でも除外／`--target` 優先
- **Verify**: `make check` パス

## [ ] Task 3 — リモート生存状態 + fetch --prune

- [ ] `src/git.rs`（fetch_prune / remote_branch_alive → Alive/Deleted/Unknown）
- [ ] `src/cleaner.rs`・`src/ui.rs` へ配線（spawn→分析→wait）
- [ ] **テスト(単体)**: `remote_branch_alive`（remote ref 有/無 fixture）
- [ ] **テスト(統合)**: `tests/remote_state.rs`（bare origin で push→削除→prune→表示）
- **Acceptance**: pruned→削除済み／残存→存在／追跡なし→不明／fetch 失敗は警告して継続
- **Verify**: `make check` パス（live fetch のみ手動）

## [ ] Task 4 — 対話削除（y/N/skip + git branch -d）

- [ ] `src/ui.rs`（prompt / Decision、reader/writer 注入）
- [ ] `src/git.rs`（delete_branch シェルアウト、stderr surface）
- [ ] `src/cleaner.rs`（ループ駆動、`--dry-run` は削除なし）
- [ ] **テスト(単体)**: `prompt`（y/n/空=N/skip/不正→再プロンプト）
- [ ] **テスト(統合)**: `tests/interactive_delete.rs`（`y\n`で消失/`n\n`で残存/`--dry-run`で削除なし）
- **Acceptance**: `y`削除／`n`スキップ／`--dry-run`削除しない
- **Verify**: `make check` パス（TTY 体感のみ手動）

## [ ] Task 5 — init コマンド（冪等テンプレート追記）

- [ ] `src/config.rs`（GLOBAL_TEMPLATE / LOCAL_TEMPLATE / has_cleaner_section）
- [ ] `src/init.rs`（`~/.gitconfig` と `repo.path()/config` に未存在時のみ追記）
- [ ] **テスト(単体)**: `has_cleaner_section`（ヘッダ検出バリエーション）
- [ ] **テスト(統合)**: `tests/init_idempotent.rs`（HOME=tempdir で2回実行→バイト一致、コメント保存）
- **Acceptance**: 初回追記／2回目 no-op／既存不変／コメント保存
- **Verify**: `make check` パス

## [ ] Task 6 — 仕上げ

- [ ] エラーメッセージ（非 repo / targets 未設定 / fetch 失敗 / 候補ゼロ）
- [ ] ヘルプ文言・exit code
- [ ] `README.md`
- [ ] **テスト(統合)**: `tests/error_paths.rs`（非 repo / targets 未設定 / 候補ゼロ）
- **Acceptance**: 各エラーパスで適切なメッセージ／fetch 失敗は継続／候補ゼロ明示
- **Verify**: `make check` パス + 各エラーパス手動スモーク

---

## チェックポイント（人間レビュー）

- [ ] Task 0 後: clap 構成の確認
- [ ] Task 1 後: 実マージで merged 判定の正しさ・`find_candidates` の seam 確認
- [ ] Task 3 後: リモート生存定義・fetch 失敗 degrade 確認
- [ ] Task 5 後: 書き込む実バイト・冪等性・コメント保存の確認
- [ ] Task 6 / リリース前: full dry-run → 使い捨て repo で実削除
