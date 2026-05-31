# git-cleaner

[![status](https://img.shields.io/github/actions/workflow/status/tominaga-h/git-cleaner/ci.yml)](https://github.com/tominaga-h/git-cleaner/actions)

ベースブランチ（`develop` / `main` 等）にマージ済みのローカルブランチを安全に検出し、ユーザー確認のもと対話的に削除する CLI ツール。

設定は独自ファイルを作らず、Git 標準の config（`[cleaner]` セクション）に統合する。

## インストール

```bash
cargo install --path .
# または開発時
cargo build --release   # target/release/git-cleaner
```

## 使い方

```bash
# git configに設定を追加
git-cleaner init

# マージ済みブランチ掃除（対話削除）
git-cleaner

# 削除対象の確認のみ（削除しない）
git-cleaner -d
git-cleaner --dry-run

# config の targets を一時的に上書き
git-cleaner -t main
git-cleaner --target develop

# 削除対象を先頭 N 件に絞る（大量のブランチがある場合に便利）
git-cleaner --limit 5
git-cleaner -d -l 10        # 先頭10件だけ dry-run で確認
```

### オプション

| オプション | 説明 |
|-----------|------|
| `-d`, `--dry-run` | 実際の削除は行わず、削除対象の一覧と詳細を表示して終了する。 |
| `-t <BRANCH>`, `--target <BRANCH>` | `cleaner.targets` を一時的に上書きするマージ先ブランチ。 |
| `-l <N>`, `--limit <N>` | 削除対象を抽出後の先頭 N 件に絞る。dry-run・対話削除の両方に適用される。 |

## 設定（Git Config 統合）

`git-cleaner init` を実行すると、以下のテンプレートが追記される（既に `[cleaner]` セクションがあれば何もしない）。

### `cleaner.targets`
マージ先として判定するベースブランチ（カンマ区切り）。

### `cleaner.protect`
削除対象から除外する保護ブランチ（カンマ区切り）。

`targets` / `protect` はグローバル設定（`~/.gitconfig`）とローカル設定（`.git/config`）の両方の値を**結合（union）**して評価する。**カレントブランチは設定の有無に関わらず常に保護される。**

```ini
# ~/.gitconfig
[cleaner]
	protect = main,master

# .git/config
[cleaner]
	targets = develop
	protect = staging
```

## 処理フロー

1. **リモート情報の最新化**: 裏側で `git fetch --prune` を実行する（失敗時は警告して継続）。
2. **設定の読み込み**: `cleaner.targets` / `cleaner.protect` を結合して取得。
3. **対象ブランチの抽出**: ターゲットに（通常マージで）マージ済みのローカルブランチを抽出し、保護ブランチとカレントブランチを除外。
4. **対話型確認・削除**: 1件ずつ提示し、`y` で `git branch -d` を実行。`n` / 空入力 / `skip` でスキップ。

表示情報: ブランチ名 / マージ先 / **マージ日時＋マージコミット短縮ハッシュ** / 最終コミット日時（相対時間併記）＋**コミットメッセージ1行目** / リモートブランチの生存状態。

> マージ日時・ハッシュは、ターゲット側でそのブランチを取り込んだマージコミット（`--no-ff` 等で作られる）を特定して表示する。fast-forward / squash などマージコミットが存在しない場合は「マージ: 日時不明」と表示する。

### 部分マージの警告

ターゲットにマージされた後、さらにブランチ側へ**ターゲット未取り込みのコミットを追加した**ブランチ（部分マージ）は、`⚠` 付きで「一部マージ済みですが、未マージのコミットがあります」と警告表示する。これらは候補には出すが、`git branch -d` の安全機構が削除を拒否するため、誤って未マージ作業を失うことはない（必要なら手動で `git branch -D`）。

### 未 push コミットの警告と force 削除

ターゲット（develop 等）にはマージ済みでも、**リモート追跡ブランチ（`origin/同名`）へ未 push のローカルコミットがある**ブランチは、`⚠ 未 push` 警告を表示する。このようなブランチは `git branch -d` が upstream 基準で削除を拒否する。

これらの候補ではプロンプトの選択肢が `(y/N/skip/force)` になり、**`force`（または `f`）を選ぶと `git branch -D`（強制削除）**を実行する。ターゲットにはマージ済みなので、コミット内容自体は失われない（ただし `origin/同名` ブランチは残る）。`y` を選んだ場合は通常の `git branch -d` を試み、git が拒否すればそのエラーを表示してブランチは残す。

## 開発

```bash
make check   # fmt-check + cargo check (-Dwarnings) + clippy + test
make fmt     # cargo fmt
```

## 制限事項（フェーズ1）

- 検出対象は**通常マージ**（マージコミットが残る形式）のみ。Squash / Rebase マージ（コミットハッシュが変わる形式）は対象外。
- `git fetch --prune` と `git branch -d` は `git` バイナリにシェルアウトする（リモート認証はユーザーの `git` 設定をそのまま利用）。
