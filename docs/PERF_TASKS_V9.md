# guff 高速化タスク 第9弾 — 第8弾の見立ては 3 つとも外れていた。AST のノードが太かった

> **前提**: `docs/PERF_TASKS.md` §0 の絶対ルール10個、`docs/PERF_TASKS_V2.md` §0 のルール11〜15、
> `docs/PERF_TASKS_V3.md` §3 の検証プロトコルは**すべてそのまま有効**です。再掲しません。
>
> **このファイルの立ち位置**: 第8弾が出した V8-2 / V8-3 / V8-4 の 3 タスクを実際に着手した回です。
> **3 つとも、着手前の見積もりが実測と合いませんでした**（0.69s の律速 → 実は 0.00s、
> 0.3〜0.5s → 実は 0.01s、analyze CPU の 20〜30% → 実は 0.7%）。
> そして**誰も見ていなかった場所に、wall −12% / CPU −18% / RSS −23% が落ちていました**。
>
> **着手前に §0 と §1 を必ず読んでください。**

**計測日**: 2026-08-17 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（`.golangci.yml`, 118 roots / 1616 pkgs）
**ベース**: `b18da95`（第8弾着手前の main）

---

## 📌 セッションを引き継いだ人はここから

> ### いちばん重要な結論 — **AST のノード 1 個が太かった**
>
> `Expr` は **192 バイト**、`Stmt` は **656 バイト**ありました。Rust の enum は
> **一番太いヴァリアントに全ヴァリアントが揃えられる**ので、24 バイトの `StarExpr` も
> 16 バイトの `EmptyStmt` も、その値段を払っていました。
>
> 太らせていた犯人は 2 つだけです: `FuncLit`（192B）と `FuncType`（136B）。
> この 2 つを `Box` に入れると:
>
> | | before | after |
> |---|---:|---:|
> | `Expr` | 192 B | **80 B** |
> | `Stmt` | 656 B | **320 B** |
> | `Spec` | 360 B | **248 B** |
>
> `Stmt` は 1 行も触っていません。`SendStmt = {Expr, Pos, Expr}` のように
> **`Expr` を埋め込んでいるものが全部連鎖して縮んだ**からです。
>
> 実測（prometheus `./...`、交互 A/B 6 ラウンド）:
>
> | | before | after | |
> |---|---:|---:|---|
> | cold wall | 2.275s | **2.130s** | −6.4% |
> | cold CPU | 12.78s | **11.48s** | −10.2% |
> | seed warm wall | 1.54s | **1.36s** | −12% |
> | seed warm CPU | 8.70s | **7.15s** | −18% |
> | peak RSS | 2490 MiB | **1926 MiB** | −23% |
> | `-j 1` wall | 5.35s | **4.93s** | −7.9% |
> | `-j 1` RSS | 2272 MiB | **1776 MiB** | −22% |
>
> **壊れた呼び出し箇所はワークスペース全体で 12 個**でした。パターンマッチは
> match ergonomics でそのまま通り、直す必要があったのは
> `Expr::FuncLit(FuncLit { body, .. })` 形の分解束縛と構築箇所だけです。
>
> ### 2 番目に重要な結論 — **`preorder CPU` は走査のコストではありません**
>
> 第5弾以降ずっと引用されてきた「preorder が analyze CPU の 28.7%」は、
> **走査 + 全アナライザのコールバック本体**の合計です。カウンタが計っているのは
> `visit_masked` で、`visit_masked` はコールバックを呼ぶからです。
>
> `GUFF_DEBUG_PREORDER_NULL=1` で走査だけを計り直すと:
>
> ```
> inspect preorder: 16744 calls, 9620311 scanned, 7375746 delivered,
>                   1.61s total CPU (27.9% of analyze CPU)
>   of which traversal only: 0.04s (2.3%); callbacks 1.58s
> ```
>
> **走査は 0.04s。** V8-4（ruff 方式の融合トラバーサル）の上限は
> analyze CPU の **0.7%** であって 20〜30% ではありません。**やらないでください。**

---

## 0. 第8弾のタスクがどうなったか

| タスク | 第8弾の見積もり | 実測 | 判定 |
|---|---|---|---|
| V8-1 アーム別カウンタ | — | — | **DONE**（landed） |
| V8-2 `format_checks` の内訳 | 「wall に効く唯一の大物」 | wall 効果 **0.00s** | **測って確定 / 部分的に着手** |
| V8-3 wide scan の 3 本 | preorder CPU −0.3〜0.5s | **−0.01s** | **実装済み・効果は誤差以下** |
| V8-4 融合トラバーサル | analyze CPU −20〜30% | 上限 **0.7%** | **NO-GO** |
| V8-5 依存型情報の遅延化 | peak RSS の主因 | seed 866 MiB < root 型検査 768+ MiB | **前提を訂正 / 別解で −23%** |
| V8-6 `--watch` に型を保持 | 1.51s → 0.05〜0.1s | 未着手 | **§4 に再掲** |

### 0.1 V8-2 — `format_checks` は律速ではありませんでした

第8弾は phase 表の `analyze 0.66s` と `format_checks 0.69s` が並んでいるのを見て
「format が analyze を追い越した」と読みました。**format は analyze とだけ重なるのではなく、
`go list` を含む run 全体と重なります**（`run_and_write_inner` がプロセス開始直後に
spawn する）。phase 行が自分で報告している `waited` がその答えです:

| fmt threads | phase ran | wall waited |
|---:|---:|---:|
| 1 | 2.03s | 0.54s |
| 2 | 1.15s | 0.00s |
| **3（既定）** | **0.64s** | **0.00s** |
| 6 | 0.43s | 0.00s |

**既定の 3 スレッドで 0.00s。約 0.9s の余裕があります。**
`format_checks` を速くしても wall は 1 ミリ秒も縮みません。

内訳は `GUFF_DEBUG_CACHE=2` で出るようになりました（V8-2 の本来の宿題）:

```
guff:     format stage CPU (summed over 3 fmt threads, not wall):
             read+filter   0.023s (1.3%)
            shared parse   0.139s (7.9%)
                     gci   0.634s (33.6%)
                 gofumpt   0.745s (40.7%)
      format (own parse)   0.312s (17.8%)   ← goimports（別モードで自前 parse）
                    diff   0.000s (0.0%)
```

gci の 3 分の 1 は**全ファイルの 2 回目の parse** でした。gci は import ブロックを
組み直して gofmt に渡し、gofmt がそれをもう一度 parse します。組み直した結果が
元のバイト列と同一でも、です。そして**必ず同一になりませんでした** —
`reconstruct` が最初の spec だけインデントせず gofmt に任せていたので、
`dist == src` は 589 ファイル全部で成立しませんでした。
最初の spec も字下げすると（gofmt がどのみち字下げし直すので gci の出力は不変）
589/589 が共有 parse を再利用します。**format CPU 1.886s → 1.750s。**

**残りをやる価値**: wall にはゼロ。CPU で ~1.7s（全体 8.7s の 20%）。
`gofumpt` の 0.745s は「fumpt ルール適用 + 印字」で、印字は避けられません。
**CPU/電力を目的に明示して着手するなら、次は goimports の自前 parse を
共有 parse に寄せる**（parser mode が `ALL_ERRORS | SKIP_STAMP_NODE_IDS` で違うので、
そこを揃えられるかの調査から）。

> **やらなかったこと（意図的）**: 「gofumpt の出力が src と一致するなら
> `gofmt(src) == src` も言えるので gci の印字は丸ごと省ける」という推論は成立します
> （gofumpt は gofmt と同じ printer config で印字し、printer は自分の出力に対して冪等）。
> ですが**gci の答えが gofumpt の結果に依存する**構造になり、
> 依存先が「printer の冪等性」という、どこにも強制されていない性質です。
> wall 効果ゼロのために入れる複雑さではないと判断しました。

### 0.2 V8-3 — アーム選択は正しくなりましたが、賞金がありませんでした

`MAX_MERGE_KINDS = 4` を「グループの実サイズで選ぶ」に変えるのは**正しい**です。
`errcheck`（6 kind / 窓の 5%）が `gocritic`（20+ kind / 窓の 42%）と同じアームに
落ちていたのは、kind の**個数**を見ていたからで、定数を上げれば gocritic が 17 倍遅くなります。

直した結果、errcheck の scanned は **1,606,518 → 88,210**（delivered は 84,173 で不変）。
copylocks と gocritic は算数どおり scan に残ります。

**ただし whole-run では見えません。** 交互 A/B 6 ラウンドで CPU **−0.3%**、
median と min が符号で食い違う＝ノイズです。第8弾は「1.6M ノードを scan するのは高い」
という前提で 0.3〜0.5s と見積もりましたが、**1.6M ノードの scan は 0.05s** でした。

副産物として `Events` を `{ptr, kind}` の配列から **2 本の配列**に分けました。
`{*const (), NodeKind}` は 8 + 1 + パディング 7 = 16 バイトで、
wide scan は kind しか読みません。分けると scan は 1 ノード 1 バイトになり、
配列自体も 44% 小さくなります。

### 0.3 V8-4 — 上限が 0.04s なので作りませんでした（ルール14）

§📌 のとおり。`GUFF_DEBUG_PREORDER_NULL=1` で走査だけを計れます。

> **実装上の注意**: この計測の最初の版はコールバックを `|_| {}` にしていました。
> LLVM は「副作用のないコールバック」を見て `NodeRef` の再構築ごと削除し、
> アームの 1 本を `group.len()` に畳みます。**0.02s と出ますが、それは
> 「走らなかった走査」の値段です。** `black_box(n)` で
> 「イベントを訪ね、使える `NodeRef` を手渡す」までを固定してください。

### 0.4 V8-5 — RSS の主因は依存の型情報ではありませんでした

`GUFF_DEBUG_RSS=1` の実測（改善前）:

```
rss now  187 MiB (seed build start)
rss now  896 MiB (seed build done, +709 MiB)   ← 依存の型情報（V8-5 の狙い）
rss now 2083 MiB (post typecheck_roots, +1186 MiB)  ← root の型検査。こちらが大きい
rss now 2445 MiB (post analyze, +363 MiB)
```

**seed が積むのは 709 MiB、root の型検査が積むのは 1186 MiB。**
V8-5 は小さい方の半分を狙っていました。そして内訳:

```
type arenas: types=545.6MiB objects=230.0MiB scopes=35.5MiB … (types_total=843.4MiB)
Info maps:   136.2MiB
AST est:     294.2MiB envelope (1606518 nodes × ~192B)
```

この `× ~192B` が §📌 の入り口です。しかも `Stmt` は 656B なので
**実際の AST はこの見積もりより大きい**（実測でも `Expr` の縮小は
envelope 見積もりの −171 MiB に対して RSS を −418 MiB 動かしました）。

`Expr`/`Stmt` を縮めた後:

```
rss now  866 MiB (seed build done)
rss now 1634 MiB (post typecheck_roots, +768 MiB)
rss now 1931 MiB (post analyze, +297 MiB)
type arenas: 843.4MiB（不変） / Info maps 136.2MiB（不変）
AST est:     122.6MiB envelope (1606518 nodes × ~80B)
```

**型アリーナ 843 MiB は 1 バイトも動いていません。** V8-5 が本当に狙うべきはここで、
それは `types=545.6MiB` の中身の話です（§3.2 に再掲）。

---

## 1. この回のベースライン（次回はこの数字を基準に）

`perf/v8` マージ後の `cargo build --release` 直後、prometheus `./...`:

| 条件 | wall | CPU | peak RSS |
|---|---:|---:|---:|
| cold（`GUFF_CACHE` 空 + `--no-cache`） | 2.13s | 11.48s | 1931 MiB |
| seed warm / issue cold | **1.36s** | 7.15s | 1926 MiB |
| 完全 warm（無変更） | 0.13s | 0.14s | 128 MiB |
| cold `-j 1` + `RAYON_NUM_THREADS=1` | 4.93s | — | 1776 MiB |

seed warm の phase 内訳と、**wall のクリティカルパス**:

```
load_graph      0.24s ┐
typecheck_roots 0.53s ├ この 3 本は直列。合計がほぼ wall。
analyze         0.60s ┘
issues+filter   0.03s
────────────────────────
format_checks   0.64s  ← 直列ではない。プロセス開始から重なり、waited 0.00s
wall            1.36s
```

> **phase 表の読み方（第8弾がここで転びました）**: `format_checks` の行は
> **クリティカルパスの上にありません**。wall を足し算で確かめるときは
> `load_graph + typecheck_roots + analyze + issues` を足してください。

---

## 2. 次にやると効くこと（優先順）

### V9-1 — `Ident` の `name: String` をインライン化する（**未着手 / 最有望**）

`Expr` を 80 バイトにした今、いちばん多いノードは `Ident`（**64 バイト**）です:

```rust
pub struct Ident {
    pub name_pos: Pos,                                            //  8
    pub name: String,                                             // 24 + ヒープ確保 1 回
    pub obj: Mutex<Option<Arc<crate::scope::Object>>>,            // 16
    pub id: u32,                                                  //  4
}
```

**識別子 1 つにつきヒープ確保が 1 回**走ります。`err` / `x` / `ctx` / `nil` のような
短い名前が大半なので、22 バイト以下をインラインに持つ文字列型にすれば
確保がまるごと消えます。プロファイルでは `mi_malloc_aligned` + `mi_free` +
`mi_page_free_list_extend` + `_mi_theap_realloc_zero` で **CPU の 6.7%** です。

**GO/NO-GO の測り方**: `Ident` の生成数（= parse したノードのうち Ident の数）を
`GUFF_DEBUG_CACHE=2` に出し、`mi_malloc` の呼び出し数と突き合わせる。
`Deref<Target = str>` を持つ型にすれば `.name == "foo"` も `format!` も通るので、
壊れるのは `ident.name = ...` の代入だけのはずです（V8 の boxing と同じ形の調査から）。

### V9-2 — `Stmt` を 320 バイトからさらに縮める（**未着手 / 小〜中**）

いまの `Stmt` の値段を決めているのは `RangeStmt`（320B: `Option<Expr>` 2 本 +
`Expr` + `BlockStmt`）です。`Stmt::Range` を `Box` に入れると次点
（`SendStmt` / `DeclStmt` / `IfStmt` 系）まで落ちます。V8 の boxing と
**まったく同じ手順**が使えます:

```bash
# 1. ast.rs のヴァリアントを Box<...> にする
# 2. cargo check --workspace --message-format short で壊れた箇所を数える
# 3. size_of を測り直して、割に合うか判断する
```

**注意**: `Expr` と違い `Stmt` の連鎖効果はもう小さいはずです。**先に size を測って、
`Stmt` を 320 → いくつにできるかを見てから**着手してください。

### V9-3 — `types=545.6MiB` の中身を割る（**未着手 / RSS の本丸**）

V8-5 の正しい入口です。型アリーナ 843 MiB のうち 545.6 MiB が type slot。
**まだ誰も「どの種類の型が何本あるか」を測っていません。**
`rss.rs` の attribution は slot 数 × slot サイズしか出しません。

- 先に `TypeId` の種類別ヒストグラム（Named / Signature / Slice / Map / …）を出す。
- `slot` 1 個のサイズと、種類ごとの重複率（同じ `[]byte` が何本あるか）を見る。
- **intern が効いていない種類があればそこが答え**で、
  第8弾が想定した mmap + 遅延展開はその後の話です。

### V9-4 — `seed dep check` の `open()`（**測って据え置き / 難物**）

seed warm の `seed dep check` は 0.34s、その CPU の **59% が `__open`** です
（19,172 個の依存ソースを開いて内容ハッシュを取る）。

**スレッドを増やしても直りません。** 実測（`GUFF_RAYON_THREADS` を 4→10）:

| threads | seed | wall |
|---:|---:|---:|
| 4（既定） | 0.33s | 1.54s |
| 6 | 0.34s | 1.53s |
| 8 | 0.35s | 1.51s |
| 10 | 0.36s | 1.53s |

グローバル rayon プールが 4 本なのは**型アリーナの RSS を抑えるため**で
（`init_rayon_global_stack`）、速度の判断ではありません。それでも増やして
効かないので、**カーネル側で直列化している**（macOS の path 解決 + sandbox チェック）
と読むのが妥当です。

残る手は「**開く数を減らす**」だけです。有力な候補:

- `$GOMODCACHE` 配下は Go が read-only + checksum 検証で不変を保証しているので、
  **内容ハッシュではなくモジュールバージョンをキーにする**。依存の大半がこれに該当します。
- **これは意味論の変更です**（module cache を手で書き換えても検出しなくなる）。
  findings は変わりませんが、キャッシュ無効化の感度が変わるので
  **ユーザーの承認を取ってから**着手してください。Go の build cache 自体が
  同じ前提に立っている、という材料はあります。

### V9-5 — `--watch` に型 / SSA を保持させる（**未着手 / 特大。第8弾 V8-6 の測り直し**）

**第8弾が置いた「現状 1.51s → 0.05〜0.1s」は、分子も分母も違います。**
1.51s は issue キャッシュを丸ごと捨てた再実行の値で、**1 ファイル直したときの値ではありません**。
実測（`tsdb/head.go` に 1 行足して、issue キャッシュは温存）:

```
phase load_graph          0.04s
phase cache setup         0.01s  (101 hits, 17 misses)   ← tsdb に依存する root が 17 本
    seed dep check        0.26s  (seed hits 1453, misses 137)
phase typecheck_roots     0.40s  (17 pkgs)
phase analyze             0.30s  (17 pkgs)
────────────────────────────────
wall                      1.10s
```

無変更なら **0.13s**（118 hits / 0 misses）なので、`--watch` が縮められるのは
この 1.10s と 0.13s の差です。そして内訳を見ると:

- **0.26s の seed dep check は常駐なら丸ごと消せます**（依存は 1 つも変わっていないのに
  19,172 ファイルを開き直して検証している）。
- **0.40 + 0.30 = 0.70s は「17 パッケージの本物の再解析」**で、
  常駐化しても消えません。パッケージ内をさらに細かく無効化しない限り残ります。

**したがって現実的な上限は 1.10s → 0.35s 級（−68%）であって、0.05〜0.1s（−95%）ではありません。**
0.05s に届くには「変更ファイルに依存する解析だけ」＝パッケージより細かい粒度が要ります。

> **設計上の綱引き（第8弾は触れていません）**: いまの `run_one_pass` は
> **わざと** `LintResult` を毎回 drop しています（"so peak RSS during watch idle does not
> stack on type artifacts"）。型と SSA を保持するというのは、
> **エディタの裏で常駐するプロセスに ~1.9 GiB を持たせ続ける**という意味です。
> V9-3（型アリーナの内訳）を先にやるべき理由がここにあります。
> **アイドル RSS の目標値を決めてから着手してください。**

工数特大で、**間違えると古い findings を返す**という最悪の回帰になります。

---

## 3. 測って「やらない」と確定したもの（第8弾 §5 に追加）

### 3.1 V8-4（融合トラバーサル）を作らない

§0.3 のとおり上限 0.04s。ルール14。
`GUFF_DEBUG_PREORDER_NULL=1` で誰でも再確認できます。

### 3.2 `format_checks` を wall のために触らない

§0.1 のとおり `waited 0.00s`。CPU を目的に明示するなら価値はあります。

### 3.3 `GUFF_FMT_THREADS` を既定の 3 から上げない

4 でも 6 でも `waited` は 0.00s のままで、wall は動きません。
analyze から CPU を奪うだけです。

### 3.4 preorder のアーム選択をこれ以上いじらない

§0.2。1.6M ノードの wide scan は 0.05s しかかかりません。

---

## 4. 次のセッションへの引き継ぎ

1. **`cargo build --release` を走らせ、`ls -l target/release/guff` の mtime を目視する**
   （第8弾 §📌 の事故。ルール11 相当として扱ってください）。
2. **`V9-1`（`Ident` の文字列）から始める。** 第8弾の boxing とまったく同じ形で、
   同じ規模の当たりが期待できる唯一の残り。
3. RSS を狙うなら `V9-3`（型アリーナの内訳）。**測るところから。**
4. `V9-4` はユーザー承認が要る意味論の変更を含みます。
5. `enum` のサイズは **`std::mem::size_of` を測る習慣**をつけてください。
   第8弾までの 7 回、誰も測っていませんでした。
