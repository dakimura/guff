# guff 高速化タスク 第8弾 — ruff と並べて測ったら、analyze はもう wall の律速ではなかった

> ## ⚠️ この回のタスクは着手済みです。結果は [`docs/PERF_TASKS_V9.md`](./PERF_TASKS_V9.md) にあります
>
> **V8-2 / V8-3 / V8-4 は 3 つとも、下に書かれた見積もりが実測と合いませんでした。**
> 下の §4 を読んで着手する前に、V9 の §0 を読んでください。
>
> | タスク | このファイルの見積もり | 実測 |
> |---|---|---|
> | V8-2 `format_checks` | 「wall に効く唯一の大物」 | wall 効果 **0.00s**（`waited` が 0） |
> | V8-3 wide scan | preorder CPU −0.3〜0.5s | **−0.01s** |
> | V8-4 融合トラバーサル | analyze CPU −20〜30% | 上限 **0.7%** → **NO-GO** |
> | V8-5 依存型情報 | peak RSS の主因 | seed 709 MiB < root 型検査 1186 MiB |
>
> **特に §2.4(b) と §4 V8-4 が乗っている「preorder が analyze CPU の 28.7%」は、
> 走査のコストではありません。** カウンタは `visit_masked` を計っていて、
> `visit_masked` はアナライザのコールバックを呼びます。走査だけを計り直すと
> **0.04s（preorder CPU の 2.3%）** です（`GUFF_DEBUG_PREORDER_NULL=1`）。
>
> 下の本文は**当時の記録としてそのまま残してあります**。数字は当時のバイナリのものです。

> **前提**: `docs/PERF_TASKS.md` §0 の絶対ルール10個、`docs/PERF_TASKS_V2.md` §0 のルール11〜15、
> `docs/PERF_TASKS_V3.md` §3 の検証プロトコルは**すべてそのまま有効**です。再掲しません。
>
> **このファイルの立ち位置**: 第1〜7弾が「guff の中を掘る」回だったのに対し、
> ここは**初めて ruff を同じ土俵で実測して、guff の残りの伸びしろを外から測り直した**回です。
> 結論は 3 つとも第7弾までの想定とズレました。**着手前に §1 と §2 を必ず読んでください。**

---

## 📌 セッションを引き継いだ人はここから

> ### いちばん重要な結論 — **analyze を速くしても wall はもう縮みません**
>
> seed warm（＝1 ファイル直したあとの再解析）の phase はこうなっています:
>
> ```
> load_graph      0.24s
> typecheck_roots 0.56s
> analyze         0.66s  ┐ この 2 本はオーバーラップしている
> format_checks   0.69s  ┘ → 律速は max(0.66, 0.69) = format_checks
> issues+filter   0.03s
> ────────────────────────
> wall            1.51s
> ```
>
> **`format_checks` が `analyze` を追い越しています。**
> つまり **analyze の CPU を 0 にしても wall は 0.03s しか縮みません。**
> 第5〜7弾がずっと analyze を削ってきた結果、**削りすぎて律速でなくなった**という状態です。
>
> **wall を縮めたいなら、これから触るべきは `format_checks` と `typecheck_roots` です。**
> analyze を削る価値は残っていますが、それは **wall ではなく CPU と RSS と `-j 1`** に対してです
> （`-j 1` は 5.4s で、そこでは analyze が支配的なまま）。
>
> ### P0 の答え — **flat inspector（V1-2）はちゃんと効いています**
>
> 「アナライザごとに AST を歩き直しているのでは」という疑いは**外れ**でした。
> `visit_masked` のアーム別カウンタ（§1.1 で追加）が出した実測:
>
> ```
> preorder arm taken (of 16744 calls):
>                  single-kind group      13678 calls (81.7%)
>                      merged groups       2193 calls (13.1%)
>                   wide linear scan        873 calls ( 5.2%)
>       recursive walk (no events)             0 calls ( 0.0%)
>   recursive walk (foreign slice)             0 calls ( 0.0%)
> ```
>
> **フォールバックはゼロ。** 95% の呼び出しが O(hits) のグループ経路に乗っています。
> 残る無駄は **`wide linear scan` の 873 呼び出し（全体の 5.2%）に完全に集中**していて、
> その正体は §1.3 のとおり **`MAX_MERGE_KINDS = 4` を超える幅広マスクを持つ 3 本**です。
>
> ### 事故報告 — **`target/release/guff` が 2 日前のバイナリのまま置いてありました**
>
> このセッションは最初、`cargo build --release` を**やらずに**既存の `target/release/guff`
> （8/14 ビルド、V6/V7 より前）で測り始めてしまい、**全部の数字が間違っていました**:
>
> | | 古いバイナリ | 実際（`b18da95` をビルド） |
> |---|---:|---:|
> | cold wall | 3.81s | **2.33s** |
> | cold CPU | 15.84s | **10.20s** |
> | analyze | 1.66s | **0.67s** |
> | preorder 破棄率 | 97% | **34%** |
>
> **§0 に 11 個目のルールとして足すべきです**:
> **「測る前に必ず `cargo build --release` を走らせ、`ls -l target/release/guff` で
> mtime が今であることを目視する。」** ルール7（他のビルドが走っていないか）の裏返しで、
> このリポジトリには `cursor-agent worker` が居るため**バイナリが勝手に古くなる**方向の事故も起きます。

**計測日**: 2026-08-16 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（`.golangci.yml`, 118 roots / 1616 pkgs）
**ベース**: `b18da95`（第7弾マージ後の main）
**比較対象**: ruff 0.15.10 / ソースは `/Users/dakimura/projects/src/github.com/astral-sh/ruff`（`672bb4edf0`）

---

## 0. この回のベースライン（全部この数字を基準にしてください）

`b18da95` を `cargo build --release` した直後のバイナリ、prometheus `./...`:

| 条件 | wall | CPU | peak RSS |
|---|---:|---:|---:|
| cold（`GUFF_CACHE` 空 + `--no-cache`） | 2.33s | 10.20s | 2559 MiB |
| seed warm / issue cold（**1 ファイル直した後**） | 1.51s | 8.90s | 2517 MiB |
| **完全 warm（無変更）** | **0.13s** | **0.14s** | **128 MiB** |
| cold `-j 1` + `RAYON_NUM_THREADS=1` | 5.48s | — | — |

phase 内訳:

| phase | cold | seed warm |
|---|---:|---:|
| load_graph (go list) | 0.49s | 0.24s |
| seed dep check | 0.83s | 0.35s |
| typecheck_roots（seed 含む） | 1.12s | 0.56s |
| analyze | 0.67s | 0.66s |
| format_checks（analyze とオーバーラップ） | 0.69s | 0.69s |
| issues+filter | 0.03s | 0.03s |

**analyze CPU は 5.90s**（全 CPU 8.90s の 66%）。

---

## 1. P0 — flat inspector は効いているのか（**答え: 効いている。DONE**）

### 1.1 V8-1 — `visit_masked` にアーム別カウンタを足した（**DONE / landed**）

`scanned` だけでは **`wide linear scan` と `recursive walk` を区別できません**
（どちらも「窓全体」を報告する）。そこで `visit_masked` が
どのアームを通ったかを返すようにし、`GUFF_DEBUG_CACHE` で出すようにしました。

- `crates/guff-analysis/src/passes/inspect.rs`: `enum Arm`（5 値）+ `PreorderCounters::arms` +
  `preorder_arm_totals()`。フォールバックは**原因別に 2 つに割ってあります**
  （`WalkNoEvents` = pass に `Arc<Package>` が無い / `WalkForeignSlice` = 呼び出し側が
  別の `[File]` を渡した）。直し方が全く違うためです。
- `crates/guff-runner/src/action.rs`: `report_preorder_timing` に出力を追加。

**コストは第2弾 B-0 の規律どおり**: 加算は `#[cold]` な `preorder_counted`（`GUFF_DEBUG_CACHE`
が付いているときだけ通る）の中だけ。release パスは未使用の enum 代入が増えるだけです。

**検証（ルール1・ルール3）**: `b18da95` の素のビルドと A/B。
**findings は `-j N` / `-j 1` の両方でバイト同一（20 件）。**
wall は `-jN` 2.22 → 2.29s、`-j1` 5.48 → 5.43s（**符号が逆＝誤差**。1 ペアのみ）。

**`regress --profile full` は PASS**（ルール6 のとおり baseline は更新していません）:

| Metric | Baseline | Measured |
|---|---:|---:|
| wall_seconds | 2.360 | **2.270** |
| peak_rss_bytes | 3,114,582,016 | **2,624,356,352** |
| guff_issues / golangci_issues / both | 20 / 20 / 20 | **20 / 20 / 20** |
| precision / recall | 1.0000 | **1.0000** |

> **baseline（2.360s / 3,114,582,016）を wall でも RSS でも下回っています**が、
> これは第6弾・第7弾の成果であって V8-1 の効果ではありません。
> **baseline の更新はユーザーの明示的承認が要ります（ルール6）。**

### 1.2 実測結果 — フォールバックはゼロ

```
guff: inspect preorder: 16744 calls, 11138708 nodes scanned, 7375746 delivered
      (34% filtered by mask), 1.69s total CPU (28.7% of analyze CPU)
guff: preorder arm taken (of 16744 calls):
                 single-kind group      13678 calls (81.7%)
                     merged groups       2193 calls (13.1%)
                  wide linear scan        873 calls (5.2%)
```

**`recursive walk` は 0 件**。`events_for` は一度も失敗していません。
V1-2（kind 別バケット）と V1-2b（`from_ref(file)` の subslice 対応）は**設計どおり動いています**。

per-analyzer 表でも `scanned == delivered` が並びます（＝1 ノードも無駄にしていない）:

```
        printf      104896 scanned      104896 delivered
        SA1019      210027 scanned      210027 delivered
        inline     1030492 scanned     1030492 delivered
```

### 1.3 残った無駄は 3 本のアナライザに完全に集中している

破棄されている 3,762,962 ノード（11,138,708 − 7,375,746）は、ほぼ全部これです:

| analyzer | mask の kind 数 | scanned | delivered | 破棄率 |
|---|---:|---:|---:|---:|
| `gocritic` (`WALKED_KINDS`) | 20+ | 1,497,576 | 625,288 | 58% |
| `copylocks` (`WANTED`) | 8 | 1,606,518 | 238,189 | 85% |
| `errcheck` (`WANTED`) | 6 | 1,606,518 | 84,173 | **95%** |

いずれも `MAX_MERGE_KINDS = 4` を超えているので `wide linear scan` に落ちています。

> **注意（このセッションが踏んだ罠）**: `grep -o "node_mask!([^)]*)"` で call site の
> kind 数を数えると **1〜3 種類しか見つかりません**。上の 3 本は `node_mask!` を
> **複数行**で書いているので、その grep には写りません。**数えるなら複数行対応で数えてください。**

---

## 2. ruff と同じ土俵で測った結果

### 2.1 ruff の実測値

対象: `~/.pyenv/versions/3.13.9/lib/python3.13/site-packages`
（**15,587 files / 7,034,986 行 / 265 MB** — 偶然ですが guff が prometheus で読む依存とほぼ同量）

| 条件 | wall | CPU | peak RSS |
|---|---:|---:|---:|
| default rules (E4,E7,E9,F), `--no-cache` | 0.69s | 4.24s | 296 MB |
| `--select ALL`, `--no-cache` | 2.79s | 14.24s | 2.14 GB |
| `--select ALL`, **cache warm** | 2.91s | 14.26s | 2.13 GB |

> **ruff の warm が cold と同じなのは仕様です。** ruff のキャッシュは
> **findings がゼロのファイルしかスキップしません**（`crates/ruff/src/diagnostics.rs:199`
> の `FileCache::linted` 判定）。`--select ALL` では大半のファイルに findings が出るので
> キャッシュはほぼ無効化されます。
> **guff のパッケージ単位 issue キャッシュ（完全 warm 0.13s）はこの点で ruff より優れています。**
> 「ruff 並」を名乗る文脈では、この 1 点は既に勝っていると書いてよいです。

### 2.2 1 行あたりのスループット

| | 対象ソース | CPU | lines/s/core |
|---|---:|---:|---:|
| ruff（ALL rules） | 7.03M 行 | 14.24s | **494k** |
| **guff の analyze だけ** | 367k 行 | 5.90s | **62k** |
| guff の依存型検査（seed + typecheck） | 7.10M 行 | ~2.5s | ~2.8M |

**差は約 8 倍**（第一報では 16.7 倍と書きましたが、それは §📌 の古いバイナリでの数字です）。
そして **guff の依存型検査は ruff より 5 倍以上速い**。第2〜7弾の投資はここに出ています。

### 2.3 依存クロージャの増幅は 19 倍（ただし効いているのは RSS とコールドスタート）

`go list -deps ./...` で実測:

| | files | 行 | bytes |
|---|---:|---:|---:|
| lint 対象（prometheus 本体） | 725 | 367k | 11.7 MB |
| **型検査が必要な依存** | **19,172** | **7.10M** | **229 MB** |

**19 倍。** ただし CPU では ~2.5s（全体の 28%）しか占めていません。
**この増幅が効いているのは peak RSS 2.5 GB と cold の typecheck_roots 1.12s です。**

### 2.4 ruff が速い構造的な理由（ソースの該当箇所つき）

**(a) 1 ファイル完結。依存グラフもトポロジカル順序もバリアも無い。**

`crates/ruff/src/commands/check.rs:83`:

```rust
let diagnostics_per_file = paths.par_iter().filter_map(|resolved_file| { ... lint_path(...) });
```

これだけです。Python の `import foo` は**名前として**解決されるだけで `foo` のソースは読みません。
**Go のリンタである guff にはこの選択肢がありません**（staticcheck 系が型を要求する）。ここは埋まりません。

**(b) AST を 1 回しか歩かない。**

`crates/ruff_linter/src/checkers/ast/mod.rs` に visitor は **1 つだけ**。
`visit_stmt` / `visit_expr` が `analyze::statement()` / `analyze::expression()` を呼び、中身は:

```rust
// crates/ruff_linter/src/checkers/ast/analyze/statement.rs:19
pub(crate) fn statement(stmt: &Stmt, checker: &mut Checker) {
    match stmt {
        Stmt::Global(...) => {
            if checker.is_rule_enabled(Rule::GlobalAtModuleLevel) { ... }
            if checker.is_rule_enabled(Rule::AmbiguousVariableName) { ... }
        }
        Stmt::FunctionDef(...) => { /* 十数個の is_rule_enabled */ }
```

`statement.rs` に **304 個**、`expression.rs` に **384 個**の `is_rule_enabled` ゲートが、
**ノード種別 match の枝に直接**並んでいます。走査は 1 回、振り分けはコンパイラの match 1 回、
ルール ON/OFF はビットセット参照 1 回。

**guff は 16,744 回の独立した走査**をしています（§1.2）。個々は O(hits) まで最適化済みですが、
**それでも preorder が analyze CPU の 28.7%（1.69s）**を占めます。これが §3 の V8-3 の対象です。

**(c) フォーマッタが自前でサブプロセスを spawn しない。**
guff も `GUFF_NATIVE_FMT` がデフォルト ON で native 化済みです（第1弾 Task 1d）。**この差は解消済み。**

---

## 3. salsa と ty の正体 — **ruff の速さの理由ではありません**

`ruff` リポジトリで salsa に依存しているクレートを列挙しました:

```
ruff_db, ruff_graph, ruff_index, ruff_python_ast, ruff_python_formatter,
ty, ty_project, ty_python_semantic, ty_python_core, ty_module_resolver,
ty_ide, ty_server, ty_test, mdtest, ruff_mdtest
```

**`ruff_linter` と `ruff`（CLI 本体）は入っていません。**
`grep salsa crates/ruff_linter/Cargo.toml crates/ruff/Cargo.toml` は何も返しません。

> **`ruff check` が速いのは salsa のおかげでは一切ありません。** §2.4 の (a)(b) が理由です。
> **「ty/salsa を入れれば速くなるはず」という方向に工数を使わないでください。**

### salsa が実際にやっていること

1. **メモ化クエリグラフ** — 関数をクエリとして登録すると結果がキャッシュされ、
   **そのクエリが読んだ入力・他クエリが自動記録**される。
2. **red-green 無効化** — 入力が変わると到達可能なクエリだけ再検証。
   「再実行したが結果が前と同じ」なら下流には伝播しない。
3. **durability** — 「めったに変わらない入力」に印を付けられる。ty は stdlib / site-packages の
   スタブを高 durability にしていて、ユーザーコード編集時は**依存エッジを辿ることすらしない**。

### ty が速い理由（salsa 以外）

**モジュール全体を先に型検査しません。**「この定義の型は?」というクエリ単位で
**遅延計算 + メモ化**します。関数本体の推論は誰かが要求するまで走りません。

### guff にとっての意味

| | ruff の linter | ty | guff の現状 |
|---|---|---|---|
| クロスファイル | しない | する（遅延） | する（**先行・全部**） |
| AST 走査回数 | 1 | — | 16,744 |
| インクリメンタル | 無し | salsa | issue キャッシュ（パッケージ単位） |

**salsa 相当が guff に効くのは常駐プロセス（`--watch` / LSP）だけです。**
一発実行の完全 warm は既に 0.13s なので、**そこに salsa を入れる理由はありません。**
一方 **ty の「遅延・オンデマンド」の発想は §4 の V8-5（RSS）に直接効きます。**

---

## 4. タスク一覧

**優先順位は「何を縮めたいか」で変わります。§📌 のとおり analyze は wall の律速ではありません。**

| 目的 | 触るべき phase |
|---|---|
| **wall（seed warm）** | `format_checks` 0.69s → `typecheck_roots` 0.56s → `load_graph` 0.24s |
| **wall（cold）** | `typecheck_roots` 1.12s → `format_checks` 0.69s → `load_graph` 0.49s（**ルール4: go list は触らない**） |
| **CPU / `-j 1`（5.48s）** | `analyze` 5.90s CPU |
| **peak RSS（2.5 GB）** | 依存の型情報（V8-5） |

### V8-2 — `format_checks` 0.69s の中を測る（**着手済み。結論: wall には効かない** → V9 §0.1）

**誰もこの中を測っていません。** `analyze` とオーバーラップしていたので今まで隠れていましたが、
**追い越した以上ここが律速です。**

- まず `GUFF_DEBUG_CACHE=2` に **gofumpt / goimports / gci の内訳**を出す
  （第6弾 V6-4 が native lister にやったのと同じ手順）。
- 725 ファイルに対して 0.69s。`GUFF_FMT_THREADS` が何本で回っているか、
  バリアがあるか（第6弾 §2.1 の wave バリアと同じ病気が無いか）を確認する。
- **仮説を立てる前に測ること。** 第6弾・第7弾の教訓です。

**期待値**: ここが半分になれば **wall −0.3s（seed warm 1.51 → 1.2s 級）**。
現時点で **wall に効く唯一の大物**です。

### V8-3 — wide linear scan の 3 本（**実装済み。効果は誤差以下** → V9 §0.2）

§1.3 の `gocritic` / `copylocks` / `errcheck`。

**`MAX_MERGE_KINDS` を上げるだけでは駄目です。** merge アームは
「k 本のカーソルから最小を線形に選ぶ」ので **O(hits × k)**。算数:

| analyzer | k | merge の比較回数 | scan の mask テスト | 勝つのは |
|---|---:|---:|---:|---|
| `errcheck` | 6 | 84,173 × 6 = 505k | 1,606,518 | **merge（3.2倍速い）** |
| `copylocks` | 8 | 238,189 × 8 = 1.91M | 1,606,518 | ほぼ互角 |
| `gocritic` | 20+ | 625,288 × 20 = 12.5M | 1,497,576 | **scan（現状のままが正しい）** |

**だから「定数を上げる」ではなく「グループの実サイズでアームを選ぶ」のが正解です。**
`kind_off` から `Σ|group_k|` は **O(k) でタダで取れます**。
`selected × log2(k) < total` なら merge（k が大きいときは二分ヒープ）、そうでなければ scan。

**期待値**: preorder CPU 1.69s のうち **0.3〜0.5s**。analyze CPU −5〜8%。**wall には出ません。**

### V8-4 — ruff 方式の融合トラバーサル（**NO-GO。上限 0.04s** → V9 §0.3）

V8-3 を入れても「アナライザごとに 1 回ずつ自分のグループを歩く」構造は残り、
**16,744 回の独立走査 = analyze CPU の 28.7%** はほぼそのままです。

**設計**（ruff の `analyze::expression` と同じ形）:
- `NodeKind → そのkindを待つアナライザのコールバック列` という逆引き表を作る。
- パッケージごとに `Events` を **1 回**走査し、各ノードでその kind の全コールバックを連続で呼ぶ。
- **融合できるのは「`inspect` だけに依存する AST 専用アナライザ」に限る。**
  `buildir` / facts に依存するものは順序制約があるので対象外。
  **最初のステップは「206 アクション中いくつが該当するか数える」こと。**

**移行**: 1 本ずつ。`GUFF_INSPECT_MASKS` と同じ発想で融合パスを env で切れるようにし、
**融合 ON / OFF で findings がバイト同一**であることを各段階で確認（ルール1）。

**期待値**: analyze CPU **−20〜30%**（5.90 → 4.2s 級）。**`-j 1` の 5.48s に直接効きます。**
wall（`-j N`）には V8-2 が終わるまで出ません。

### V8-5 — 依存の型情報を遅延化して RSS を下げる（**前提を訂正。狙う先が違う** → V9 §0.4 / V9-3）

19 倍の増幅（§2.3）が peak RSS 2.5 GB に出ています。
第7弾が確定させたとおり **macOS では解放しても RSS は戻らない**ので、
**「最初から commit しない」以外に手はありません。**

**ty の写像**: seed overlay を「全部デシリアライズして持つ」のをやめ、
**mmap + 実際に参照された export だけ遅延展開**する。

**注意**: CPU の旨味は薄い（seed は既に 0.35s）。
**RSS ゲートと、prometheus より大きいリポジトリでの実行可能性のための投資**と位置づけること。

### V8-6 — `--watch` に型 / SSA を保持させる（**未着手 / 特大** → V9-5 に再掲）

第2弾の `--watch` MVP は型と SSA を保持していません。保持すれば
**1 ファイル変更時にそのファイルに依存する解析だけ**再実行できます。
**salsa（あるいは既存の dep-hash レジストリをメモリ上で回す形）が本当に効く唯一の場所です。**

**現状 1.51s → 0.05〜0.1s 級。** ただし工数特大で、
**間違えると古い findings を返す**という最悪の回帰になります。V8-2 と V8-4 の後。

---

## 5. 測って「やらない」と確定したもの

### 5.1 salsa / ty を一発実行パスに入れない

§3 のとおり ruff の linter は salsa を使っていません。
一発実行の完全 warm は既に 0.13s で **ruff の同条件（2.91s）より速い**。
**salsa は常駐（V8-6）専用の道具として扱ってください。**

### 5.2 `MAX_MERGE_KINDS` を単に大きくしない

§4 V8-3 の表のとおり、`gocritic`（k=20+）では merge の方が **8 倍遅くなります**。
`inspect.rs` の既存コメント「Raising this is a measurement, not a guess」は正しい。
**アーム選択をグループ実サイズに基づかせるのが正解です。**

### 5.3 依存を export data 経路に切り替えない

**ルール5 のまま。** 今回の実測でも依存型検査は **~2.5s CPU で 7.10M 行**
（ruff の 5 倍以上のスループット）。ここは既に速く、`go list -export` は袋小路です。

### 5.4 analyze をこれ以上「wall のために」削らない

**§📌 のとおり `format_checks` に追い越されています。**
analyze を削る作業は **CPU / `-j 1` / RSS のため**と明示して着手してください。
「wall が縮むはず」で始めると、測ったときに 0.03s しか動かなくて混乱します。

---

## 6. 次のセッションへの引き継ぎ

1. **`cargo build --release` を走らせ、`ls -l target/release/guff` の mtime を目視する**（§📌 の事故）。
2. **`V8-2`（format_checks の内訳計測）から始める。** wall に効く唯一の大物で、まだ誰も測っていない。
3. CPU / `-j 1` を狙うなら `V8-3` → `V8-4`。
4. `V8-1`（アーム別カウンタ）は landed。`GUFF_DEBUG_CACHE=2` で出ます。
