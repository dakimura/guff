# guff 高速化タスク（ruff並を目指す）— 実装エージェント向け詳細手順書

> このファイルは「初めてこのリポジトリを触るエージェント」が、事故らずに
> パフォーマンス改善を進めるための手順書です。**上から順に、飛ばさず読むこと。**
> 特に §0（絶対ルール）と §1〜§3（計測と検証）は、どのタスクを始める前にも必読。
> 分からなくなったら勝手に判断せず、この手順書の該当節に戻ること。

---

## 0. 絶対ルール（違反したら作業は失敗とみなす）

このプロジェクトの過去の失敗は、ほぼ全部この10個のどれかを破って起きた。

1. **findings（検出結果）を絶対に変えてはいけない。**
   高速化は「速くする」だけ。検出される lint 結果が1件でも増減・移動したら失敗。
   検証方法は §2。**「たぶん同じ」は禁止。バイト単位で同一を機械で確認する。**

2. **RSS（ピークメモリ）を増やしてはいけない。**
   ゲートは baseline × 1.20 で落ちる。並列化・キャッシュは往々にしてメモリを食う。
   常に §1 の `/usr/bin/time -lp` の `maximum resident set size` を見る。

3. **`-j 1`（逐次）と `-j N`（並列）の両方で必ず wall を測る。**
   過去に「並列化したら逆に遅くなった」事故が複数回。並列専用パスにだけ重い処理
   （O(n²) スケジューラ等）を足すと、逐次より遅くなる。両方測って比較する。

4. **`go list`（load_graph フェーズ）には手を出さない。**
   調査済みで、コストの ~100% は Go の `go list` サブプロセスそのもの。guff 側の
   オーバーヘッドはゼロ。ここを速くしようとするのは時間の無駄。

5. **`GUFF_DEP_SOURCE=0`（export-data 経路）をデフォルトにしない／それで速くしようとしない。**
   実測で cold 22.9s（`go list -export` が Go コンパイルを誘発）かつ検出数が変わる
   （459→407）。袋小路。現状の hybrid-from-source がデフォルトで正しい。

6. **baseline を勝手に更新しない（`--update-baseline` を勝手に叩かない）。**
   ゲートは「今の baseline より悪化していないか」を見る。改善が出ても baseline 更新は
   **ユーザーの明示的な承認が要る**。承認前は「旧 baseline に対して PASS」を確認するだけ。

7. **計測前に必ず他のビルド／エージェントが走っていないか確認する。**
   このリポジトリでは `cursor-agent worker` が不定期に `cargo build`（全コア）を撃ち、
   計測を汚染する（12s の真値が 22s に化けた実績あり）。計測直前に必ず:
   ```bash
   ps aux | grep -iE 'cargo|rustc|cursor-agent' | grep -v grep
   ```
   ビルドが走っていたら終わるまで待つか、再計測する。

8. **pointer identity（`ptr as *const _ as usize`）を map のキーにしているコードで、
   そのノードを `.clone()` してはいけない。** 過去に非決定的な false-positive を生んだ。
   自分が触る解析器がこのパターンを使っていないか grep で確認。

9. **1タスク = 1論点。** 複数のタスクを同時に混ぜない。1タスク終わるごとに §1・§2 の
   検証を通してからコミットする。混ぜると、findings が変わったとき原因の切り分けが不能になる。

10. **findings が変わったら、それは「バグを直した」のではなく「あなたが壊した」。**
    高速化タスクで検出が変わることは原則ない（速くするだけだから）。変わったら即ロールバック
    して原因を探す。「新しい方が正しいのでは」と自己判断で baseline を動かさない。

---

## 1. 計測ハーネス（コピペで動く。毎回これで測る）

### 1.1 前提
- 対象コードベースは同梱の prometheus チェックアウト（リポジトリ直下の `prometheus/` シンボリックリンク）。
- release ビルドで測る（debug ビルドの数字は無意味）。
- マシン想定: Darwin arm64 / 10-core。Linux の場合 `/usr/bin/time` のフラグが違う（§1.5）。

### 1.2 ビルド
```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff
cargo build --release            # target/release/guff ができる
```

### 1.3 cold 計測（regress ゲートと同じ条件: warm GOCACHE + 空の linter cache）
```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff/prometheus
GUFFBIN=/Users/dakimura/projects/src/github.com/dakimura/guff/target/release/guff
CACHE=$(mktemp -d)
GUFF_CACHE="$CACHE" GUFF_DEBUG_CACHE=1 /usr/bin/time -lp \
  "$GUFFBIN" run --no-cache -c .golangci.yml ./... \
  >/tmp/guff_out.txt 2>/tmp/guff_dbg.txt
grep -iE 'phase |seed dep|typecheck_roots |analyze \(|format_checks|real |maximum resident|hits=' /tmp/guff_dbg.txt
echo "issues: $(grep -c '\.go:' /tmp/guff_out.txt)"
rm -rf "$CACHE"
```

### 1.4 warm 計測（繰り返し実行。キャッシュ hot。ruff並を狙う主戦場）
```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff/prometheus
GUFFBIN=/Users/dakimura/projects/src/github.com/dakimura/guff/target/release/guff
CACHE=$(mktemp -d)
# 1回目でキャッシュを温める（--no-cache を付けない！）
GUFF_CACHE="$CACHE" "$GUFFBIN" run -c .golangci.yml ./... >/dev/null 2>/dev/null
# 2回目を計測
GUFF_CACHE="$CACHE" GUFF_DEBUG_CACHE=1 /usr/bin/time -lp \
  "$GUFFBIN" run -c .golangci.yml ./... >/dev/null 2>/tmp/warm.txt
grep -iE 'phase |real |maximum resident|hits=' /tmp/warm.txt
rm -rf "$CACHE"
```

### 1.5 Linux の場合
`/usr/bin/time -lp` → `/usr/bin/time -v`（`Maximum resident set size (kbytes)` を見る）。
`regress/measure.py` が両対応しているので、細かい計測は §3 のゲート経由が確実。

### 1.6 `GUFF_DEBUG_CACHE=1` が出す phase の読み方
```
phase load_graph (go list) 1.15s     ← §0-4: 触らない
phase typecheck_roots 3.55s          ← うち seed build 3.11s が最大（Task 4 の対象）
phase analyze (run_on_packages) 1.89s ← buildir/testifylint が重い（Task 5 の対象）
phase issues+filter 0.46s            ← Task 3 の対象
phase format_checks 0.83s            ← サブプロセス。Task 1 の対象
cache setup+partition 0.60s          ← warm でのみ重い。Task 2 の対象
```
`per-analyzer analyze time` 表は **worker 横断の合計** であって wall ではない
（並列で走るので実 wall はもっと短い）。ここを勘違いして「analyze が 10s」と誤読しないこと。

### 1.7 現状の基準値（2026-07-22 実測、prometheus `./...`, 10-core）

| シナリオ | wall | RSS | 備考 |
|---|---:|---:|---|
| cold（改善前） | 8.25s | 7.7GB | |
| cold（Task 3/2/5 後） | 7.79s | 7.7GB | issues+filter 0.49→0.06s; testifylint skip |
| cold seed-hot（Task 4 後, GUFF_CACHE 永続） | ~5.0s | 7.4GB | seed build 3.3→**0.5s**。空キャッシュ cold は ~8.0s で不変 |
| warm 繰り返し（改善前） | 2.04s | 0.22GB | |
| warm 繰り返し（Task 3/2/5 後） | 1.22s | 0.18GB | cache setup 0.60→0.18s (dep-hash hit) |
| warm 繰り返し（Task 1 fmt_check cache） | **0.44s** | 0.18GB | format_checks 0.85→**0.07s** |

各タスクの「before/after」はこの表と、自分の環境で測り直した数字で比較する。

**Task 1 状況（2026-07-22）:** 1a/1b/1c/1e バイト一致 ✅。warm は
`${GUFF_CACHE}/fmt_check/v1` で format_checks **0.85→0.07s**（findings 冷/温同一、
決定性3回 OK、regress tsdb PASS）。**cold hybrid の前提が更新された:** 以前は
「in-process 全件 format は遅い」ため cold も `-l` hybrid だったが、`node_size` の
メモ化バグ（`self.node_sizes.clone()` して破棄＝超二次）を Go 同様の共有マップ方式
（`fprint_with_sizes` が map を返し `mem::take` で往復）に直した結果、native 全件が
**1737ms→161ms（~10.8×）**、prometheus 725 で **native 161ms < `gofumpt -l` 180ms**
と逆転。cold の `-l` を native list に差し替える道が開けた（未実施＝下記「残り」）。
**併せて parser の param-grouping バグ（`expr_eq_shallow` が clone 済み型を ptr::eq で
比較 → 常に非グルーピング）を構造比較に修正**し、既存の偽陽性を除去（gocritic
paramTypeCombine「統合済みを統合せよ」+ 誤出力由来の gofumpt）: **tsdb 76→74、
full 460→424**（追加0・真の検出/recall 不変）。baseline 更新済み。
**ネイティブ list への差し替え DONE 2026-07-23:** cold の `-l`/`gci list`
サブプロセスを廃止し、`runner::native_list`（各ファイルを in-process で `format()` し
差分のあるものだけを flag、par_iter）に置換。default-native の gofmt/gofumpt/gci が
対象（goimports は native が format-only=Task 1d 未了のためサブプロセス `-l` 継続）。
findings バイト同一（full 424/408/4/16・tsdb 74・決定性3回・`-j 1` も同一）、
両 regress PASS。gofumpt/gci をダミーに差し替えても findings 不変＝サブプロセス非 spawn を確認。
効果は控えめ（cold format_checks ~0.78→0.74s、wall はノイズ内）だが gofmt/gofumpt/gci の
外部ツール依存を除去（残る format サブプロセスは goimports `-l` のみ）。残り: goimports add-remove。
（Task 4 = seed 永続化は **DONE 2026-07-22**。）

Task 1b: gofmt both **6333/6333**。1c gofumpt prometheus **725/725**。1e gci **725/725**。
`GUFF_NATIVE_FMT=0` で format をサブプロセスに、`--no-cache` で fmt_check も無効。

## 2. findings 同一性の検証（毎タスク必須。これを通さずにコミット禁止）

**原理:** 高速化の前後で、guff の検出結果を JSON で出して、ソートして diff を取る。**空 diff = OK。**

### 2.1 手順
```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff/prometheus
GUFFBIN=/Users/dakimura/projects/src/github.com/dakimura/guff/target/release/guff

# 変更「前」の出力を取る（コードを変える前に、または git stash して）
gen() {
  local out="$1"; local cache; cache=$(mktemp -d)
  GUFF_CACHE="$cache" "$GUFFBIN" run --no-cache -c .golangci.yml --out-format json ./... 2>/dev/null \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); \
print("\n".join(sorted(f"{i[\"Pos\"][\"Filename\"]}:{i[\"Pos\"][\"Line\"]}:{i[\"Pos\"][\"Column\"]}:{i[\"FromLinter\"]}:{i[\"Text\"]}" for i in (d.get("Issues") or []))))'
  rm -rf "$cache"
}
gen /tmp/before.txt
# ... ここでコードを変更してビルド ...
gen /tmp/after.txt
diff /tmp/before.txt /tmp/after.txt && echo "FINDINGS IDENTICAL ✅" || echo "FINDINGS CHANGED ❌ ロールバックせよ"
```
> `--out-format json` のフィールド名（`Pos`/`Filename`/`FromLinter`/`Text`）は golangci-lint 互換。
> 実際のキー名が違ったら `guff run --out-format json ./... | head` で1件の構造を確認して直す。
> 出力件数だけの比較（`grep -c`）は**不十分**。位置や文言が入れ替わる回帰を見逃す。必ず diff を取る。

### 2.2 決定性の確認（3回回して同一か）
非決定性バグ（並列化で順序が揺れる等）を炙り出すため、**同じ条件で3回**回して
`after.txt` が毎回同一になることを確認する。1回でも違ったら非決定性を埋め込んでいる。

### 2.3 これが本番ゲート（§3）でも自動チェックされる
`regress/run.sh` は guff と golangci-lint の findings 差集合を比較し、
`guff_only` / `golangci_only` の増分が **0 を超えたら FAIL**（`regress/gate.py`,
tolerances: `max_guff_only_delta=0`, `max_golangci_only_delta=0`, `min_both_delta=0`）。

---

## 3. 本番ゲート（コミット前に必ず PASS させる）

```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff
./regress/run.sh                 # tsdb プロファイル（軽い・速い。まずこれ）
./regress/run.sh --profile full  # ./... 全体（本番。重い）
```
ゲートの合格条件（`regress/gate.py`）:
- wall ≤ baseline × **1.25**
- peak RSS ≤ baseline × **1.20**
- `guff_only` 増分 = 0 / `golangci_only` 増分 = 0 / `both` 減少なし

現 baseline（`regress/baseline*.json`、2026-07-22 Task 4 後に再ロック）:
- tsdb: wall 2.54s / RSS 1.42GB / guff_issues 76
- full: wall 7.95s / RSS 7.62GB / guff_issues 460

**両プロファイルが PASS しなければコミットしない。** 改善が出ても baseline は
**ユーザー承認まで更新しない**（§0-6）。承認が出たら
`./regress/run.sh [--profile full] --update-baseline` でロック。

---

## 4. タスク一覧・推奨順・依存関係

価値（wall 削減）は 1 > 4 > 2 > 5 > 3 だが、**リスク（findings を壊す確率）と工数**を
加味した「頭の悪いエージェントでも事故りにくい推奨着手順」は以下:

| 順 | タスク | 効く所 | 期待削減 | 工数 | リスク | 独立性 |
|---|---|---|---|---|---|---|
| 1 | **Task 3**: issues+filter 調査 | warm/cold | 0〜0.4s | 小 | 低 | 独立 |
| 2 | **Task 2**: dep-hash キャッシュ | warm | ~0.4s | 中 | 中 | 独立 |
| 3 | **Task 5**: buildir/testifylint 条件スキップ | cold | ~0.5s | 中 | 中 | 独立 |
| 4 | **Task 4**: seed 永続化 ✅**DONE** | cold(hot) | ~2.7s | 大 | 高 | 独立 |
| 5 | **Task 1**: ネイティブフォーマッタ | warm/cold | ~0.7s | 特大 | 最高 | 独立（サブ分割）|

各タスクは互いに独立（別々の phase を触る）。**必ず1つ完了→検証→コミットしてから次へ。**
自信がなければ Task 3 → 2 → 5 の順で「小さく確実な勝ち」を積む。Task 1・4 は大物なので
時間と注意力に余裕があるときだけ着手。

---

## Task 3 — issues+filter フェーズの調査と最適化（まずこれ）

### 目的
warm 0.44s / cold 0.46s を占める `issues+filter` フェーズに、二乗ループ等の無駄がないか
調べ、あれば潰す。**なければ「無かった」と報告して終わってよい**（無理に変えない）。

### 対象ファイル
- 出力箇所: `crates/guff-lint/src/lib.rs`（`phase issues+filter` を grep）
- フィルタ本体: `crates/guff-analysis/` 内の exclude / nolint 処理。まず
  `grep -rn 'nolint\|exclude\|filter' crates/guff-analysis/src crates/guff-runner/src | head` で特定。

### 手順
1. `phase issues+filter` の計測コードの位置を確認（`crates/guff-lint/src/lib.rs`）。
2. フィルタ処理の中で「issue 1件ごとに全ルール／全パスを線形スキャン」していないか読む。
   典型的な地雷: `issues.iter().filter(|i| rules.iter().any(|r| ...))`（O(issues × rules)）、
   `paths.iter().position(...)`、正規表現をループ内で毎回コンパイル。
3. 見つけたら、事前にインデックス化（HashMap/HashSet）、正規表現の事前コンパイル、
   パス正規化のメモ化などで潰す。**アルゴリズムを変えるだけ。判定結果は1ビットも変えない。**

### 検証（必須）
- §2.1 findings diff = 空。
- §2.2 3回回して同一。
- §1.3/1.4 で `issues+filter` の秒数が下がった（or 少なくとも増えていない）ことを確認。
- §3 の `./regress/run.sh` と `--profile full` 両方 PASS。

### やってはいけない
- exclude/nolint の**意味**を変える（＝findings が変わる）。これは高速化ではない。
- 「たぶん無駄」で当てずっぽうに書き換える。プロファイル（`sample` / debug 表）で裏を取る。

### ロールバック基準
findings が1件でも変わったら即 `git checkout -- <file>`。原因が分かるまでコミットしない。

---

## Task 2 — dep-hash レジストリのディスクキャッシュ（warm 高速化）

### 目的
warm 繰り返し実行で `cache setup+partition` が 0.60s かかる。これは**毎回** 1792 パッケージの
dependency ハッシュを全再計算しているため。`go list` 出力が変わっていなければ前回結果を
再利用して、この phase を ~0.1s 以下にする。

### 背景（読まないと事故る）
- キャッシュの整合性は「dep-hash」に依存している。過去に**非決定的な dep-hash** が
  warm キャッシュを半分ミスさせる致命バグを起こした。dep ハッシュは必ず
  **`go list` の flat な `Package::deps`（ソート済み・完全）** から、全パッケージの
  `id/pkg_path → self_hash` レジストリに対して計算する（`crates/guff-runner/src/cache.rs`,
  `IssueCache::set_dep_hashes`）。imports の Arc グラフを再帰してはいけない（解決深さが
  実行ごとに揺れて非決定的になる）。
- つまり **今キャッシュしようとしている dep-hash レジストリ自体が、決定的でなければならない。**

### 対象ファイル
- `crates/guff-runner/src/cache.rs`（dep-hash レジストリ構築 `set_dep_hashes`）
- 呼び出し元: `crates/guff-lint/src/lib.rs`（`cache setup+partition` phase）

### 手順
1. まず `cache setup+partition` の 0.60s の内訳を確認する。debug タイミングを一時的に
   足して「レジストリ構築」「per-package の cache 照合」のどちらが重いか切り分ける。
   （レジストリ構築が主因である前提で以下を書くが、違ったら報告して方針を相談）。
2. `go list` の出力全体（またはパッケージ id/deps の連結）から**決定的な**サマリハッシュ
   `graph_key` を作る。パッケージは必ず id でソートしてから連結する（HashMap 順序禁止）。
3. `${GUFF_CACHE}/dep_hash_registry.<graph_key>.bin` に前回のレジストリを保存。次回、
   同じ `graph_key` のファイルがあればロードして再利用、無ければ再計算して保存。
4. `graph_key` が変わる = 依存グラフが変わった = 全再計算。これで正しさは保たれる。

### 検証（必須）
- **決定性が命。** §2.2 を **5回** 回して findings が毎回同一。1回でも違えば dep-hash が
  非決定的 → 即ロールバック。
- warm を2回計測し、2回目で `cache setup+partition` が激減していること（§1.4）。
- **キャッシュ無効化が効くことの確認**: prometheus のどれか1ファイルを編集 → その
  パッケージとその依存元だけが再チェックされ、findings は編集内容に正しく追従する。
  （編集を戻して findings が元に戻ることも確認）。
- RSS が baseline × 1.20 以内（§3）。ディスクキャッシュのロードでメモリを二重持ちしない。
- `./regress/run.sh` と `--profile full` 両方 PASS。

### やってはいけない
- HashMap のイテレーション順に依存した連結・フォーマット（§0-8 の親戚。過去の最頻事故）。
- レジストリを imports グラフから作る（非決定的。必ず flat `deps` から）。
- キャッシュファイルの破損時にクラッシュ（壊れていたら黙って再計算にフォールバック）。

### ロールバック基準
findings が揺れた／warm が速くならない／RSS 超過 → `git checkout`。

---

## Task 5 — testify 非依存パッケージで buildir/testifylint をスキップ（cold 高速化）

### 目的
cold の analyze 1.89s のうち、worker 合計で buildir 2.65s・testifylint 2.41s が突出。
buildir は SSA を構築する重い解析で、その主な消費者は testifylint。**testify を import
していないパッケージでは testifylint は何も検出しない**ので、そこで buildir + testifylint を
まるごと省ける（他に buildir を使う解析が無いパッケージに限る）。

### 背景（重要な前提確認）
- buildir の結果を testifylint 以外の解析器も使っているかを **必ず先に確認する**。
  ```bash
  grep -rn 'buildir\|BuildIr\|Ssa\|SSA' crates/guff-*/src | grep -iE 'depend|require|input|need' | head
  ```
  testifylint 以外に buildir 依存の解析器（例: いくつかの staticcheck SA チェック）が
  あれば、**それらのどれかが動く可能性のあるパッケージでは buildir をスキップしてはいけない**。
  「testifylint だけが唯一の消費者」でない限り、条件は「buildir 依存解析が**全て**
  スキップ可能」に厳しくすること。ここを雑にやると findings が消える（＝重大回帰）。
- スキップ条件の判定は**安価な import スキャン**で行う（型チェック前に分かる）。
  パッケージの import に `github.com/stretchr/testify`（および buildir を使う他解析の
  トリガ）が1つも無ければスキップ。

### 対象ファイル
- 解析スケジューラ: `crates/guff-runner/src/action.rs`（`dependency_waves` / action グラフ）
- buildir: `crates/guff-govet/` or `crates/guff-buildir/`（`ls crates/ | grep -i ir` で特定）
- testifylint: `grep -rl testifylint crates/*/src | head`

### 手順
1. 上記「背景」の依存確認を**先に**やり、buildir の全消費者を列挙する。
2. パッケージ単位で「buildir 依存解析が1つも走らない」ことを import から判定する述語を作る。
3. その述語が真なら、そのパッケージの buildir action と依存 action をスケジュールしない。
4. 判定は import 文字列の集合照合のみ（型情報不要・安価）にする。

### 検証（必須・特に厳格に）
- §2.1 findings diff = **空**。testifylint / SA チェックの検出が**1件も消えていない**ことを
  必ず確認（このタスクは「検出を消す」方向の回帰が最も起きやすい）。
- テスト用に、testify を使うパッケージ（prometheus 内に多数ある）で testifylint の検出が
  従来どおり出ることを名指しで確認する。
- §1.3 cold で analyze フェーズ秒数が下がったことを確認。
- RSS はむしろ下がるはず（SSA を作らない分）。増えていたらおかしい。
- `./regress/run.sh` と `--profile full` 両方 PASS。

### やってはいけない
- 「testify を使ってなさそう」を型情報で判定しようとする（型チェック後になり、そもそも
  高速化にならない上に複雑）。import 文字列だけで判定する。
- buildir の消費者を testifylint だけと決めつける（§背景。必ず grep で全消費者を確認）。

### ロールバック基準
検出が1件でも減ったら即ロールバック。「その検出は元々ノイズ」等の自己判断は禁止（§0-10）。

---

## Task 4 — guff 自前 export seed のディスク永続化（cold 最大の勝ち・大物）

> ✅ **DONE 2026-07-22 — 既定 ON**（`GUFF_SEED_PERSIST=0` で無効化）。
> 実装: `crates/guff-packages/src/seed_cache.rs`（`OverlayWriter` / `pkg_self_hash_from_sources` /
> `load_overlay` / `overlay_path` / `base_fingerprint`）+ `build_source_seed`（`crates/guff-packages/src/typecheck.rs`）
> + `WorkerOverlays::{encode,decode,clear_source_positions}` / `SEED_OVERLAY_SCHEMA`（`crates/guff-types/src/check.rs`）。
> 各 source dep の exported-API overlay を `${GUFF_CACHE}/seed/<pathkey>.<self_hash>.<base_fp>.v<schema>.bin` に保存し、
> 依存が変わっていなければ decode+remap して再利用（型チェックをスキップ）。
> **結果:** GUFF_CACHE 永続運用で seed build **3.34→0.48s**、wall **8.0→5.0s**。findings は cold(全ミス)↔hot(全ヒス)
> でバイト同一（§2.1 diff 空、5回決定的、`-j 1` も同一）。破損/欠落/スキーマ不一致は黙って再構築。
> **miss パスは実質ゼロコスト**: ソースは1回だけ読んで hash とパーサで共用（逐次事前パス廃止）、
> ディスク書込は `OverlayWriter` バックグラウンドスレッドでクリティカルパス外。空キャッシュ cold の
> overhead は +0.19s（誤差）で、両 regress プロファイル PASS。
> ⚠️ 過去の「既定 ON は空キャッシュ cold に ~4s 乗る」報告は **計測汚染（§0-7 の並行 cargo build）による誤り**。
> 真値は ~0.67s で、上記2点の修正で ~0.19s まで削減済み。

### 目的
cold の `typecheck_roots seed build` が 3.11s で全 phase 中最大。これは 1455 個の依存
パッケージを**毎回ソースから型チェック**しているコスト。各パッケージの「型チェック済み
exported API」をディスクにシリアライズし、内容ハッシュをキーに再利用すれば、依存が
変わっていない cold 実行で seed build を ~0.4s（decode+merge）に落とせる。
メモリの "facts persistence across runs" 積み残しの本命。

### なぜ今なら現実的か（前提）
- 型アリーナの id を持つ全構造体には既に `remap_ids`（id 再配置）が実装されている
  （`crates/guff-types/src/merge.rs` の `Remapper`、wave-parallel マージ導入時に整備）。
  シリアライズ後に別実行へロードする際の id 再配置がこの機構でできる。
- `ExportSeed` / `WorkerOverlays`（`crates/guff-types/src/check.rs`）が per-package の
  overlay を既に扱っている。永続化の単位はこの overlay。

### 対象ファイル（読む順）
1. `crates/guff-packages/src/typecheck.rs`（`build_source_seed` / `typecheck_roots`。seed 構築の中枢）
2. `crates/guff-types/src/check.rs`（`ExportSeed` / `Checker::from_seed` / `merge_wave`）
3. `crates/guff-types/src/merge.rs`（`Remapper` / `remap_ids`）
4. `crates/guff-types/src/arena.rs`（`Layered` / `Id::remapped`）
5. `crates/guff-runner/src/cache.rs`（dep-hash レジストリ＝キャッシュキーの作り方）

### 設計方針
1. **キャッシュキー = パッケージの self-hash + guff のスキーマバージョン。**
   self-hash は Task 2/既存の dep-hash レジストリと同じ決定的ハッシュを使う。
   **スキーマバージョン定数**を1つ設け、型アリーナの構造やシリアライズ形式を変えたら
   必ずインクリメントする（古い形式を読んでクラッシュ／誤動作を防ぐ）。
2. **シリアライズ対象 = 1パッケージ分の overlay（exported API のみ）。**
   関数本体は `IgnoreFuncBodies` で既に落ちている（seed は exported API だけ）。それを保存。
3. `${GUFF_CACHE}/seed/<pkg_self_hash>.<schema_ver>.bin` に保存。
4. seed build 時、依存ごとに: キャッシュがあれば decode → `remap_ids` で id を現在の
   アリーナ空間へ再配置してマージ。無ければ従来どおりソースから型チェックして、結果を保存。
5. シリアライズは `serde` + `bincode` 等。ただし **id フィールドの再配置漏れが致命的**
   （§落とし穴）。

### 落とし穴（過去のマージ実装で踏んだもの・必読）
- **id を持つフィールドの remap 漏れ = サイレント破損。** wave-merge 実装時、
  `ObjectMeta.parent` / `ObjectMeta.pkg` のような「見落としやすい id フィールド」で
  実際に踏んでいる。`remap_ids` が**全 id 保持構造体**（14 TypeData variants、
  7 ObjectData variants、Scope、Package、TypeList/TypeParamList、TypeSet+TermList、
  Interface の `tset` キャッシュ）を漏れなく処理していることを再確認。永続化で新たに
  シリアライズする経路でも同じ網羅性が要る。
- **型 identity は構造的（origin ObjectId ベース）で、instance TypeId ではない。**
  クロス実行でロードしても identity が壊れないのはこの原理のおかげ。ここを誤解して
  TypeId で同一判定するコードを足すと壊れる。
- **RSS 二重持ち。** decode したアリーナと base seed を同時に全部持つと RSS が跳ねる。
  wave ごとに decode→merge→drop する現行のフロー（peak を1〜数 wave 分に抑える設計）を
  崩さない。`SEED_PARSE_CHUNK` の意図（peak resident dep-AST を1チャンクに束縛）を読む。

### 検証（必須・最重要タスクなので最も厳しく）
- **findings バイト同一**を「件数」ではなく §2.1 の**ソート済み全 issue の diff = 空**で確認。
  過去メモに「count は 460 だが grep は 459」という**正規化差**の罠がある。件数一致を
  ゴールにしない。ゲートの normalizer（`compat/normalize.py`）ベースの §3 を正とする。
- §2.2 を **5回以上**。id 再配置バグは確率的に出るので回数を増やす。
- **キャッシュ有無で findings 同一**: `${GUFF_CACHE}/seed/` を消した cold と、seed
  キャッシュ hot の cold で、findings が完全一致すること。ここが本タスクの心臓。
- **スキーマバージョン**: 古い形式のファイルを置いた状態でも、バージョン不一致なら
  黙って再計算にフォールバックする（クラッシュしない）ことを確認。
- **壊れたキャッシュ**: seed ファイルを途中で切って壊しても、クラッシュせず再計算に
  落ちることを確認。
- RSS ≤ baseline × 1.20。decode 経路で二重持ちしていないか §1.3 の `maximum resident` で確認。
- `./regress/run.sh` と `--profile full` 両方 PASS。
- seed hot の cold で `seed build` が 3.11s → ~0.4s 付近に落ちたことを確認。

### やってはいけない
- 関数本体まで保存する（不要・巨大・RSS 破壊）。exported API（overlay）だけ。
- スキーマバージョンを付け忘れる（後日フォーマットを変えた瞬間に全ユーザーが壊れる）。
- 「だいたい同じ型が復元できた」で満足する。型グラフの1エッジのズレが findings を変える。

### ロールバック基準
findings 不一致・非決定・RSS 超過のいずれか → 即ロールバック。この機能はフラグ
（例 `GUFF_SEED_PERSIST=0` で無効）で入れ、既定で無効にしてから段階的に有効化するのが安全。

---

## Task 1 — ネイティブ Rust フォーマッタ（warm/cold の最大級・特大・最高リスク）

### 目的
format_checks は warm で 0.83s（warm 全体の 40%！）・cold でも 0.83s。中身は
gofumpt / goimports / gci の**外部サブプロセス spawn**。ruff の本質は「フォーマッタも
自前・依存プロセスゼロ」。これを Rust ネイティブ実装に置き換え、サブプロセスを消す。

### 最重要の前提認識（これを軽視すると必ず壊す）
- guff には **Go ソースを出力する printer が無い。** `crates/guff-ast/src/print.rs` は
  **デバッグ用の AST ダンパー**であって gofmt 出力ではない。**ゼロから go/printer 相当を
  書く必要がある。** これは Go 本体の `go/printer` + `gofumpt` の追加規則 + `goimports` の
  import 管理 + `gci` の import グルーピングを移植する、という**特大タスク**。
- フォーマッタは**バイト単位で本物と一致**しなければ findings（"File is not properly
  formatted" の行番号）が変わる。近似は許されない。**これが本手順書で最も findings を
  壊しやすいタスク。**

### だから「差分テストハーネス」を最初に作る（コード実装より先）
本物のツールと自作実装の出力を、大量の実ファイルでバイト比較し続ける仕組みを**先に**作る。
これが無いまま実装を進めるのは禁止。

```bash
# 差分ハーネスの考え方（gofumpt の例）:
#   1. prometheus 全 .go ファイル + Go 標準ライブラリの .go を収集
#   2. 各ファイルを「本物の gofumpt」と「自作実装」の両方に通す
#   3. バイト単位で diff。1バイトでも違えば FAIL とファイル名を出す
#   4. 全ファイル PASS になるまで実装は未完成
find /Users/dakimura/projects/src/github.com/dakimura/guff/prometheus -name '*.go' > /tmp/gofiles.txt
# （+ $(go env GOROOT)/src 配下の *.go も足すと網羅性が上がる）
# 各ファイルで:  diff <(gofumpt < f) <(guff-native-gofumpt < f)
```
このハーネスを `crates/guff-fmt/tests/` の integration test か、`regress/` 配下の
スクリプトとして常設し、CI/ローカルで回せるようにする。

### サブ分割（1つずつ・この順で。各サブタスクは独立にリリース可能）
実装は formatter 単位で分割し、**1つ完成→差分ハーネス全 PASS→findings 検証→コミット**を
繰り返す。全部を一度に書かない。

- **Task 1a: 差分テストハーネス構築** ✅**DONE 2026-07-22**
  （`regress/fmt_diff.py` + `guff-fmt-native` + `regress/tests/test_fmt_diff.py`。
  `--self-check` で参照ツール idempotence、通常モードで native vs 参照のバイト比較。
  native 未実装時は exit 3 / `--allow-not-implemented` で soft PASS）。
- **Task 1b: ネイティブ gofmt** ✅**DONE 2026-07-22**（`go/printer`+`go/format`+`text/tabwriter` 移植。
  prometheus+GOROOT **6333/6333** バイト一致。`Gofmt` 既定ネイティブ。`-s` は未移植→サブプロセス）。
- **Task 1c: ネイティブ gofumpt** ✅**DONE 2026-07-22**（gofmt + gofumpt 追加規則。
  prometheus `extra-rules: true` → `gofumpt --extra` 725/725 PASS。`format()` 既定ネイティブ）。
- **Task 1d: ネイティブ goimports** 🟡**PARTIAL**（format-only が prometheus 725/725。
  import add/remove 未実装のため Formatter 既定はサブプロセス。`GUFF_NATIVE_FMT=1` で opt-in）。
- **Task 1e: ネイティブ gci** ✅**DONE 2026-07-22**（prometheus sections 725/725。
  `format()` 既定ネイティブ）。

> **Check-mode: native list（2026-07-23、`-l` 廃止）+ fmt_check cache:**
> `list_unformatted` は default-native の gofmt/gofumpt/gci では
> `runner::native_list`（in-process `format()` して差分ファイルだけ flag、par_iter）に
> なり、cold prefilter のサブプロセス `-l`/`gci list` を spawn しない。`GUFF_NATIVE_FMT=0`
> のときのみ従来のシステム `-l` にフォールバック。goimports は native が format-only
> （Task 1d 未了）のため既定サブプロセス `-l` のまま。加えて `${GUFF_CACHE}/fmt_check/v1`
> に結果を永続化し、warm 2回目以降は list 自体をスキップ（format_checks **0.85→0.07s**、
> findings 冷温同一）。`--no-cache` で無効。
> **経緯（2026-07-22）:** `node_size` 二次バグを解消し native 全件 format が `gofumpt -l`
> を上回った（161ms < 180ms / 725 files）ため「native が遅いから `-l`」の前提が失効し、
> 上記の native list 差し替えが可能になった（実施済み）。

### 対象ファイル
- 置換対象: `crates/guff-fmt/src/{gofmt,gofumpt,goimports,gci}.rs`（現状すべて
  `Command::new` でサブプロセス spawn。`format()` と `list_unformatted()` を実装している）。
- trait: `crates/guff-fmt/src/lib.rs` の `Formatter`（`format()` を自前実装に差し替え、
  `list_unformatted()` は自前 `format()` ベースの実装に。サブプロセス不要になる）。
- 呼び出し: `crates/guff-fmt/src/runner.rs`（`check` / `batch_list`）。

### 段階導入戦略（安全のため）
- 各ネイティブ実装は**フラグ**（例 `GUFF_NATIVE_FMT=1`）の裏に入れ、既定は従来サブプロセス。
- 差分ハーネスが全 PASS した formatter から順に、既定をネイティブへ切り替える。
- **1つでも差分ハーネスが FAIL する formatter は既定にしない**（サブプロセスのまま残す）。
  部分適用でも warm は速くなる。

### 検証（必須）
- 各 formatter: 差分ハーネスが prometheus + GOROOT の**全ファイルでバイト一致**。
- §2.1 findings diff = 空（"File is not properly formatted" の行が1つも変わらない）。
- §1.4 warm で format_checks が 0.83s → 大幅減。
- サブプロセスが消えたことの確認（`strace`/`dtruss` は不要、`Command::new` 経路を通らない
  ことをコードとログで確認）。
- `./regress/run.sh` と `--profile full` 両方 PASS。

### やってはいけない
- 差分ハーネス無しで実装を進める（必ず壊す）。
- 「ほぼ一致」で妥協する（1バイト差が findings 差）。一致しないものは既定にしない。
- gofumpt/goimports/gci を gofmt 土台なしで先に書く（順序を守る: 1b → 1c → 1d/1e）。
- import 解決（goimports）で行き詰まって全体を止める。詰まったらそのサブタスクだけ
  サブプロセスに残し、他の formatter のネイティブ化を先に取り込む（部分適用可）。

### ロールバック基準
差分ハーネス FAIL / findings 変化 → その formatter は既定をサブプロセスへ戻す。

---

## 5. コミット前チェックリスト（毎回・全項目 YES で初めてコミット）

- [ ] §0 の絶対ルールに1つも違反していない
- [ ] §1.7 で他ビルド/エージェントが走っていないことを確認して計測した
- [ ] §2.1 findings diff = 空（件数一致ではなく diff が空）
- [ ] §2.2 同条件3回（Task 2/4 は5回）で findings が毎回同一（決定性 OK）
- [ ] `-j 1` と `-j N` の両方で wall を測り、並列が逐次より遅くなっていない
- [ ] RSS が baseline × 1.20 以内
- [ ] 狙った phase の秒数が実際に下がった（下がっていないなら入れる意味がない＝再考）
- [ ] `./regress/run.sh` PASS
- [ ] `./regress/run.sh --profile full` PASS
- [ ] baseline は更新していない（更新はユーザー承認後のみ）
- [ ] 1コミット = 1タスク（他タスクを混ぜていない）

---

## 6. 現実的なゴールと「ここまでで十分」の線引き

- `go list`（cold 1.15s / warm 0.14s）は固有コストで消せない。**真の 0.1s は cold では不可能。**
- 現実解: **warm ≤ 0.4s / cold ≤ 4〜5s**。
- 効いた順に取り込み、各タスク後に §1.7 の表を更新して進捗を可視化する。
- 迷ったら「速さより findings 同一が常に優先」。findings を守れないなら、その高速化は入れない。
