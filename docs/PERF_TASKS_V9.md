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
> 実測（prometheus `./...`、交互 A/B 6 ラウンド）。**この表は `Expr` の
> boxing だけの効果**です（型アリーナのぶんは後述、合計は §1）:
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
> ### 同じ話が型アリーナにもありました — `TypeData` 112 → 72 B
>
> RSS の最大項は **type slot 545.6 MiB**で、`rss.rs` はこれを
> `capacity × size_of::<TypeData>()` として数えています。**約 5.1M slot** です。
> 埋めているのは `Slice`（**4 バイト**）や `Pointer`（**4 バイト**）のような小さい種類なのに、
> `Interface`(112B) と `Named`(96B) がインラインなせいで全部が 112 バイト払っていました。
>
> この 2 つを `Box` に入れて **peak RSS さらに −150 MiB**（wall / CPU は中立）。
> **壊れた呼び出しは 2 箇所**。`Box` は serde に透過なので seed overlay の
> ディスク表現は変わりません。詳細は §V9-3。
>
> ### つまり、この回で本当に学ぶべきこと
>
> **要素数が百万単位の配列に入る型は、`std::mem::size_of` を測ってください。**
> 第1〜8弾の 8 回、誰も測っていませんでした。**同じ話が 3 回続けて出ました**:
>
> | | before | after | 壊れた箇所 | 効果 |
> |---|---:|---:|---:|---|
> | `Expr` | 192 B | 80 B | 12 | wall −12% / CPU −18% / RSS −23% |
> | `TypeData` | 112 B | 72 B | 2 | RSS −150 MiB（wall/CPU 中立） |
> | `ObjectData` | 104 B | 64 B | 1 | RSS −38 MiB（wall/CPU 中立） |
> | `Stmt`（V9-2） | 320 B | 168 B | 3 | RSS −66 MiB（wall/CPU 中立） |
>
> どれも「一番太いヴァリアント 1〜2 個が、全ヴァリアントの値段を決めていた」で、
> 直し方も同じ（`Box` に入れる）、壊れる箇所も 1〜12 個と小さい。
> **`Expr` と `TypeData` には const assert を置いて再膨張を build エラーにしてあります。**
>
> **効果は逓減しています**（−23% → −150 MiB → −38 MiB）。
> 4 つ目を探す前に §4.1 の注意書きを読んでください。
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
それは `types=545.6MiB` の中身の話です。そして開けてみると
**`Expr` とまったく同じ問題**でした（§V9-3）: `capacity × size_of::<TypeData>()` の
`TypeData` が 112 バイトあり、`Interface` と `Named` を `Box` に入れて 72 バイト、
**peak RSS さらに −150 MiB**。残りは `ObjectData`（104 B × 約 2.3M）と、
intern の重複率（未計測）です。

---

## 1. この回のベースライン（次回はこの数字を基準に）

この回のぶん（#10 / #11 / #12）を全部入れた main を `cargo build --release` した直後、
静かなマシンで prometheus `./...`（3 回の中央値）:

| 条件 | wall | CPU | peak RSS |
|---|---:|---:|---:|
| cold（`GUFF_CACHE` 空 + `--no-cache`） | **1.98s** | 10.5s | **1779 MiB** |
| seed warm / issue cold | **1.36s** | 8.5s | **1717 MiB** |
| 完全 warm（無変更） | 0.13s | 0.14s | **121 MiB** |
| cold `-j 1` + `RAYON_NUM_THREADS=1` | **4.90s** | — | **1590 MiB** |

第8弾の同じ表（`b18da95`）との差:

| | 第8弾 | 第9弾 | |
|---|---:|---:|---|
| cold wall | 2.33s | 1.98s | −15% |
| seed warm wall | 1.51s | 1.36s | −10% |
| seed warm peak RSS | 2517 MiB | 1717 MiB | **−32%** |
| `-j 1` wall | 5.48s | 4.90s | −11% |

RSS の内訳（`GUFF_DEBUG_RSS=1`, post analyze）:

```
type arenas: types=350.7MiB objects=145.1MiB scopes=35.5MiB names=8.5MiB
             intern=23.5MiB  (types_total=563.8MiB)
Info maps:   136.2MiB
AST est:     122.6MiB envelope (1606518 nodes × ~80B)
attributed:  839.6MiB of 1743 MiB
```

> **次に大きいのは、attribution が名前を付けられていない 900 MiB です**
> （SSA IR / allocator / stack）。`types=350.7MiB` はまだ大きいですが、
> slot サイズで削れる分はもう取り切りました — 次は**本数**（intern の重複率）の話で、
> それは §V9-3 のとおり未計測です。

seed warm の phase 内訳と、**wall のクリティカルパス**:

```
load_graph      0.23s ┐
typecheck_roots 0.57s ├ この 3 本は直列。合計がほぼ wall。
analyze         0.53s ┘   （うち seed dep check 0.33s は typecheck_roots の内数）
issues+filter   0.03s
────────────────────────
format_checks   0.55s  ← 直列ではない。プロセス開始から重なり、waited 0.00s
wall            1.36s
```

> **phase 表の読み方（第8弾がここで転びました）**: `format_checks` の行は
> **クリティカルパスの上にありません**。wall を足し算で確かめるときは
> `load_graph + typecheck_roots + analyze + issues` を足してください。

---

## 2. 次にやると効くこと（優先順）

### V9-1 — `Ident` の `name: String` をインライン化する（**測って NO-GO / 2026-11-28。§3.5**）

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

> **2026-11-28 追記 — 作って測ったら効きませんでした（§3.5）。** 代わりにプロファイルで
> 見つかった **AST の deep clone（CPU の 4.8%）** を §V9-6 にまとめ、一部を直しています。

### V9-6 — 熱い経路で AST を deep clone しない（**一部完了 / 2026-11-28**）

V9-1 の確認のために `samply` で seed warm を取り直したところ、`mi_malloc` / `mi_free` 系の
6.8% は**どこか 1 箇所に集まっていません**（呼び出し元の最大が printer の 0.47%）。
代わりに、`Expr` / `Stmt` / `FuncDecl` などの `Clone::clone` を**包含時間**で数えると
**CPU の 4.8%** ありました。呼び出し元の上位:

| 呼び出し元 | 包含 CPU | 何を clone していたか | 状態 |
|---|---:|---|---|
| `parser_resolver::{declare, walk_func_decl, short_var_decl, …}` | 約 1.5% | `ObjDecl` に入れる宣言ノード。`declare` が**識別子ごとに `decl.clone()` して元を捨てる**ので、`FuncDecl` は本体ごと 2 回、`var a, b, c` は 4 回、`x, err :=` は文が 2 回 | **完了**: `ObjDecl` を `Box` → `Arc`。1 宣言 1 clone |
| `guff_ssa::builder::build_syntactic_body` / `if` / `for` | 約 0.5% | `self.stmt(&Stmt::BlockStmt(s.body.clone()))`。入れ子の本体を**深さの回数だけ**コピー | **完了**: `Builder::block_stmt(&BlockStmt)` |
| `guff_types::return_check::{is_terminating, has_break}` の `if` | 小 | 同じ `Stmt::BlockStmt(s.body.clone())` の形 | **完了** |
| `ssautil::load::build_package_for_analysis` | 0.51% | `builder/mod.rs` の `f.syntax_decl = Some(fd.clone())`（全関数の `FuncDecl` を本体ごと） | **完了**: 読み直すのはジェネリック関数のインスタンス化だけなので、型パラメータを持つ関数だけ `Arc` で保持。他の読み手 2 つ（purity の「ソースがあるか」と nilness の `func` 位置）は新設の `syntax_pos` を見る。prometheus cold で instructions −0.53%、peak RSS −46 MiB（交互 3 ペア、全ペア同符号）、findings 同一 |
| `guff_ssa::builder::Builder::address` | 約 0.55% | `emit_call` / `assign` から来る `Expr` の clone | **完了**: lvalue（`Address` / `LazyAddress` / `LazyIndexAddr`）が `Option<Expr>` の代わりに `Copy` の `DebugExpr`（id・pos・reflect 名・blank か）を持つ。`emit_debug_ref` が読むのはこれだけ。staticcheck の SA4006/SA4031 が有効だと buildir は `GLOBAL_DEBUG` で組むので「debug 無効時だけ clone を省く」では主経路に効かない。prometheus cold 交互 3 ペア: instructions −0.3%（全ペア同符号、1 回目の A は warm-up で除外）、RSS 中立、findings 58 件同一。DebugRef 列は全 lvalue 形の単体テストで main と一致（そのテストの doc に、以前からある go/ssa との差を書いた: 合成リテラル要素スロットの DebugRef が出ない） |
| `gofumpt::fumpter::apply_file` | 0.39% | `FuncDecl` の clone | **完了**: 出どころは `join_lone_decls` の `old[i].clone()`。結合しない宣言（`FuncDecl` の本体ごと）も含め**ファイルの全宣言**を毎回コピーしていた。`old` を `Vec<Option<Decl>>` にして各宣言を 1 回だけ move（`break` で抜ける経路は取らずに残すので、止めた宣言が次の起点になる）。結合する `GenDecl` の二重 clone も消えた。prometheus cold 交互 4 ラウンド（gofumpt 有効）: instructions −0.4%（138.9G→138.4G、warm-up の 1 回目を除く全ペア同符号）、RSS 中立、findings 58 件同一。全分岐の形を `gofumpt_join_lone_decls.*.go` の単体テストで旧実装とバイト一致 |
| `facts::deprecated::run` | 約 0.5% | `Decl` の clone と `File` の drop | **完了**: 出どころは `run` の `pass.files().to_vec()`（`&mut pass` を呼びながら歩くために**パッケージの全 AST を毎回 deep clone**）。`Pass::files()` を `&self` ではなく pass の寿命 `'a` で借用を返すようにして、コピーをやめた。prometheus cold 交互 4 ラウンド（1 回目の A は warm-up で除外）: instructions −0.35%（146.9G→146.4G、全ペア同符号）、peak RSS −35 MiB（全ペア同符号）、findings 174 行バイト一致 |
| `govet::loopclosure::walk_stmt` | — | `Stmt::BlockStmt(s.body.clone())` の同じ形（プロファイル上位には出ていない） | **完了（互換修正として）**: 手書きの walk ごと、上流の `ast.Inspect` 相当（`preorder_prune`）に置き換えた。fixture を書いたら walk が 24 件中 11 件を取りこぼしていたため。詳細は COMPAT-HARDENING §4 2026-11-29 |

**完了分の効果**（prometheus `./...`、main `3e44c88` 比、`/usr/bin/time -l` の
**instructions retired**、交互 3 ラウンド。ばらつきは ±0.1G）:

| | before | after | |
|---|---:|---:|---|
| seed warm | 105.6 G | **103.4 G** | **−2.1%** |
| cold | 142.2 G | **139.9 G** | **−1.6%** |

wall / CPU / peak RSS は交互 6 ラウンドで向きは揃っていますが（seed warm CPU −3.9%、
cold CPU −1.7%）、計測時の load average が 3.5 前後あり、この差は揺れの幅に入ります。
findings は 174 行でバイト一致。

> **測り方のメモ**: macOS の `/usr/bin/time -l` は `instructions retired` を出します。
> 負荷のある機械でも 3 ラウンドで ±0.1%（wall は ±10%）に収まるので、
> **「仕事を減らす」変更の GO/NO-GO にはこれを先に見てください。**
> 並列度・ロックの変更は instructions に出ないので、そちらは従来どおり wall で。

### V9-2 — `Stmt` を 320 バイトからさらに縮める（**完了 / 2026-11-28。320 → 168 B、RSS −66 MiB**）

**見立ては半分外れていました。** 「`RangeStmt` を `Box` に入れれば次点まで落ちる」と
書いていましたが、ヴァリアント別に測ると次点は `SendStmt` ではなく **`DeclStmt`（312 B）**でした。
`Decl` が `FuncDecl`（`FuncType` をインラインで持つ）を抱えているからで、
**文の位置の `DeclStmt` は `GenDecl` しか持てない**のにその値段を払っていました。
`RangeStmt` だけ箱に入れても 320 → 312 B にしかなりません。

| ヴァリアント | size | | ヴァリアント | size |
|---|---:|---|---|---:|
| **`RangeStmt`** | **320** | | `IncDecStmt` | 96 |
| **`DeclStmt`** | **312** | | `LabeledStmt` / `ExprStmt` / `BranchStmt` / `TypeSwitchStmt` | 80 |
| `SendStmt` | 168 | | `GoStmt` / `DeferStmt` / `CaseClause` | 72 |
| `IfStmt` / `ForStmt` | 160 | | `AssignStmt` | 64 |
| `SwitchStmt` | 152 | | ほか | ≤ 56 |

2 つを `Box` に入れて **`Stmt` 320 → 168 B**（`ast.rs` に `STMT_STAYS_SMALL` ≤ 176 を追加）。
**壊れた箇所は 3 つ**（parser の構築 2、`s1021` の分解束縛 1）。

実測（prometheus `./...` cold、`--no-cache` + 毎回新しい `GUFF_CACHE`、交互 A/B 6 ラウンド・順序を毎回反転、
load 2.5〜3 の状態で）:

| | A（main） | B | |
|---|---:|---:|---|
| peak RSS（平均） | 1890 MiB | **1824 MiB** | **−66 MiB（−3.5%）**、6 ペアすべて同符号 |
| instructions retired（初回除く平均） | 138.47 G | 138.18 G | −0.2% |
| wall / user+sys | | | 中立（ラウンドごとの機械のドリフトの方が大きい） |

issue 集合は 12 回とも同一（`Pos.Offset` はファイルセット内の位置で実行ごとに変わるので除いて比較）。

**この先は割に合いません。** 次点の `SendStmt`（168）を箱に入れても 160、その下の
`IfStmt` / `ForStmt` は数が多く、1 文ごとの確保に見合わない。`Stmt` はここで打ち止めとします。
`Decl`（312 B）自体はファイル直下の `Vec<Decl>` にしか並ばないので触っていません。

### V9-3 — 型アリーナの slot（**半分着手済み。`TypeData` は 112 → 72 に縮めました**）

V8-5 の正しい入口です。型アリーナ 843 MiB のうち 545.6 MiB が type slot で、
`rss.rs` はこれを **`capacity × size_of::<TypeData>()`** として数えています。
つまり `Expr` とまったく同じ形の問題でした:

| | before | after |
|---|---:|---:|
| `TypeData` | 112 B | **72 B** |

`545.6 MiB / 112 B` ＝ **約 5.1M slot**。そしてアリーナを埋めているのは
**小さい種類**です（`Slice` は 4 バイト、`Pointer` も 4）。
それが 112 バイトを占めていたのは、`Interface`(112) と `Named`(96) が
インラインだったからです。この 2 つを `Box` に入れて **peak RSS −150 MiB**
（`-j N` 1932 → 1782 MiB、`-j 1` 1785 → 1643 MiB）。attribution でも:

```
before: type arenas: types=545.6MiB objects=230.0MiB … (types_total=843.4MiB)
after:  type arenas: types=350.7MiB objects=230.0MiB … (types_total=648.6MiB)
```

**壊れた呼び出しは 2 箇所だけ**でした（アリーナがよく閉じている）。
wall と CPU は交互 A/B 2 回でどちらも符号が安定せず＝中立。
`Box` は serde に透過なので **seed overlay のディスク表現は不変**です。

**残り**:

- **`ObjectData` も同じでした（104 → 64 B、1 行 / 呼び出し 1 箇所）。**
  ヴァリアント別に測ると `Const` だけが 104 で、他は全部 ≤ 56:

  | | Var | Func | TypeName | **Const** | Nil | Builtin | PkgName |
  |---|---:|---:|---:|---:|---:|---:|---:|
  | size | 56 | 48 | 48 | **104** | 48 | 56 | 56 |

  `Const` を `Box` に入れて `ObjectData` は **64 バイト**、
  attribution の `objects` は **230.0 → 145.1 MiB**。

  > **ただし peak RSS は −38 MiB しか動きません**（1760 → 1722 MiB）。
  > **retained と peak は別物です。** 箱に出た `Const` 本体は 3.6 MiB しかなく
  > （attribution が数えるようにしました）、差の大半は
  > 「peak は analyze 中の一過性の確保で決まる」ことによります。
  > **`objects=` の減り幅を peak RSS の減り幅として引用しないでください。**
- `capacity` と `len` の差（Vec の倍々成長の余り）も未計測。ただし第7弾の
  「macOS では解放しても RSS は戻らない」があるので `shrink_to_fit` は期待薄です。
- 種類ごとの重複率（同じ `[]byte` が何本あるか）は依然未計測で、
  **intern が効いていない種類があればそこが次の答え**です。
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

### 3.5 V9-1（`Ident.name` のインライン文字列化）を入れない — 2026-11-28

自前の 24 バイト型（22 バイト以下をインライン、それ以上は `Box<str>`）を作り、
ワークスペース全体を通しました（壊れた箇所は 129 ファイル／約 250 行。
`HashMap<String, _>::get(&ident.name)` と `Vec<String>` への `collect` が大半で、
§📌 の「壊れるのは代入だけ」という見積りは外れました）。findings はバイト一致。

効果（prometheus `./...`、交互 6 ラウンドの中央値）:

| | 差 |
|---|---:|
| seed warm wall / CPU / RSS | ±0.0% / −0.4% / +0.2% |
| cold wall / CPU / RSS | ±0.0% / −0.7% / +0.8% |

**揺れの幅の中です。** 前提が違っていました: プロファイルの malloc 6.7% は `Ident` 由来ではなく、
呼び出し元が数百に散っていて、`Ident::clone` は 0.05% しかありません。
1.6M ノード中の `Ident` 数十万個 × 確保 1 回 ≈ 数十 ms で、合計 CPU 8〜16s の中では見えません。
129 ファイルの変更に見合わないので revert しました。

---

## 4. 次のセッションへの引き継ぎ

1. **`cargo build --release` を走らせ、`ls -l target/release/guff` の mtime を目視する**
   （第8弾 §📌 の事故。ルール11 相当として扱ってください）。
2. ~~`V9-1`（`Ident` の文字列）から始める。~~ **2026-11-28 に測って NO-GO（§3.5）。**
   代わりに **§V9-6 の表の「未着手」行**（~~`syntax_decl` の `FuncDecl` clone 0.51%~~ 完了、
   `Builder::address` の `Expr` clone 約 0.55% など）から。どれも「clone をやめて借用する」
   だけで、findings が変わらないことは prometheus の出力のバイト比較で確かめられます。
3. RSS を続けるなら `V9-3` の残り＝ **`ObjectData`（104 B × 約 2.3M）のヴァリアント別サイズ**。
   これも同じ手順で 30 分で判断がつくはずです。
4. `V9-4` はユーザー承認が要る意味論の変更を含みます。勝手に入れないでください。
5. `V9-5`（`--watch`）は**アイドル RSS の目標値を決めてから**。
   いまの実装が型を毎回捨てているのは事故ではなく設計です。

### 4.1 この回の手順（そのまま再利用できます）

百万単位で並ぶ型を疑うときの手順は毎回同じでした。**30 分で GO/NO-GO が出ます。**

```bash
# 1. サイズを測る（テストに 1 本足して --nocapture で出すだけ）
#    println!("{}", std::mem::size_of::<TargetType>());
# 2. ヴァリアント別に測って、最大を決めている 1〜2 個を特定する
# 3. その 1〜2 個だけを Box<...> にする
# 4. 壊れた箇所を数える
cargo check --workspace --message-format short 2>&1 \
  | grep -E "error(\[|:)" | grep -v "could not compile" | sort -u
# 5. サイズを測り直し、findings 同一性 → A/B（wall / CPU）→ RSS の順で確認する
```

実績: `Expr` は 12 箇所、`TypeData` は **2 箇所**。パターンマッチは match ergonomics で
そのまま通るので、壊れるのは**構築箇所と分解束縛だけ**です。

> **`Box` にする判断は必ず実測で**。`Expr` は wall/CPU/RSS の 3 つとも改善しましたが、
> `TypeData` と `ObjectData` は **RSS だけ**で wall/CPU は中立でした。間接参照が増える以上、
> 「アクセスが熱いのに要素数が少ない」ヴァリアントを箱に入れると損をします。
>
> **そして attribution の減り幅を peak RSS の減り幅として引用しないこと。**
> `ObjectData` は attribution で −85 MiB、peak RSS で **−38 MiB** でした。
> retained（保持し続ける量）と peak（一過性の確保を含む最大値）は別の量です。
> `objects=` は箱に出た本体も数えるようにしてあるので、**この差はもう箱のせいではありません。**
