# guff 高速化タスク 第4弾 — 「山が無くなった後」の測り方と、その結果

> **前提**: `docs/PERF_TASKS.md` §0 の絶対ルール10個、`docs/PERF_TASKS_V2.md` §0 のルール11〜15、
> `docs/PERF_TASKS_V3.md` §3 の検証プロトコルは**すべてそのまま有効**です。再掲しません。
>
> **このファイルの立ち位置**: 第3弾が analyze を −40% したあと、直列 3 phase
> （load_graph / typecheck_roots / analyze）がほぼ同じ大きさになり、
> 「ここを削れば効く」という単独の山が消えました。第4弾は
> **「山が無いときに何を測るか」**を扱います。実際、今回の結論の半分は
> **「掘っても出ない」ことを測って確定させた**ことです。

---

## 📌 セッションを引き継いだ人はここから

> ### 結果（2026-08-14, ブランチ `perf-v4` / PR #5, 計測時のベース `53943d4`）
>
> **prometheus `./...` empty-cold: 2.635s → 2.530s（wall −4.0%）/ CPU 14.35s → 13.61s（−5.1%）。**
> **analyze phase 0.95s → 0.84s（−11.6%）。peak RSS 3.386 → 3.299 GiB（−87 MiB, 減った）。**
> findings は **20 件バイト同一**（並列 / `-j 1` / masks off / 3 連続同一）。
>
> 入ったものは **1 本だけ**です（§V4-1）。コメント再パース 8 箇所を
> `PARSE_COMMENTS` → `COMMENTS_ONLY`（= `| SKIP_OBJECT_RESOLUTION`）にしました。
>
> **測って「やらない」と確定させたものが 3 本あります。同じ穴を掘らないでください。**
> - **§4.1 B-9（wave バリア撤廃）は最終 NO-GO。** 「バリアが遅い」のではなく
>   **seed dep check がそもそも並列化しない**（4→10 スレッドで wall −0.04s / CPU **+1.2s**）。
> - **§4.2 O(候補 × AST) はもう残っていません。** プロファイルで裏を取った結果、
>   inspector を通らない walk の最大値が 0.22s（revive の *共有* walk）。
> - **§4.3 アロケータと `Expr::clone`/`drop` は「犯人」ではなく「結果」。**
>   最大の呼び出し元チェーンが 0.039s で、散らばっていて掴む所がありません。
>
> **`regress --profile full` が初めて PASS しました（§7.2）。** wall **2.440s** に対し
> limit 2.510s。V3 の宿題「baseline 2.360s が古いので `--update-baseline` を回すか
> 2.51s を切るか」は、**baseline を動かさずに切る側で解けました。更新は不要です。**
> ただし余裕は 0.07s しかありません。
>
> **見つけた既存バグが 1 本あります（§5）: C-7 seed speculation は 2026-07-31 以降
> 一度も発火していません。** native lister が既定になったとき、peek 先の
> `golist/` ディレクトリを誰も書かなくなったためです。**未修理**。

**計測日**: 2026-08-14 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（`.golangci.yml`, 118 roots / 1616 pkgs）, cold, `--no-cache`
**計測時のベース**: `53943d4`
**現在のベース**: `f9af656`（PR #3 / #4 / #6 マージ後の main に rebase 済み）

> **rebase で数字は動きません。** `53943d4` → `f9af656` の 3 コミットは
> テストファイル・CI ワークフロー・golden fixture と、**prometheus では latent だった
> SA4032 の修正**（PR #6）です。実際に確かめました:
> **`53943d4` のバイナリと `f9af656` のバイナリで prometheus `./...` の findings は
> バイト同一**（20 件）。よって本ファイルの A/B（すべて `53943d4` 比）はそのまま有効です。
> rebase 後に再検証したゲートは §7 に反映してあります。

---

## 1. 地図（第4弾の着手時、`53943d4`）

`GUFF_DEBUG_CACHE=2`、3 サンプル中央値。**空 `GUFF_CACHE`（毎回 `mktemp -d`）**。

| phase | wall | CPU | 並列度 | 備考 |
|---|---:|---:|---:|---|
| startup | 0.00s | — | — | |
| load_graph（native list） | 0.54s | 0.99s | 1.8× | **CPU の 72% が open/stat/getdirentries** |
| cache setup+partition | 0.00s | — | — | |
| **typecheck_roots** | **1.10s** | **2.4s** | **2.2×** | seed dep check 0.79s + target check 0.28s |
| **analyze** | **0.95s** | **8.3s** | **8.7×** | |
| issues+filter | 0.05s | — | — | |
| format_checks（並走） | 0.63s | ~1.9s | 3× | **`waited=0.00s`。余裕 ~2s。触らない** |
| **real** | **2.65s** | **13.93s** | | |

### 1.1 CPU 1 秒の価値は phase ごとに 4 倍違う

**これが第4弾で一番重要な数字です。**

| phase | 並列度 | wall −0.1s に必要な CPU 削減 |
|---|---:|---:|
| typecheck_roots | 2.2× | **0.22s** |
| analyze | 8.7× | **0.87s** |

同じ「CPU を 1 秒削る」でも、typecheck では wall −0.45s、analyze では wall −0.11s です。
**プロファイルの self CPU 順に潰すと、この 4 倍を無視することになります。**

ただし §4.1 のとおり、typecheck 側は **CPU を削っても wall に出ない**（syscall bound）ため、
実際に使えるレバーは analyze 側でした。**「レバー比が大きい」と「レバーが動く」は別の話です。**

---

## 2. 第4弾で足した計測ツール

### 2.1 `seed wave schedule` 行（`GUFF_DEBUG_CACHE=2`）

`crates/guff-packages/src/typecheck.rs`。seed の wave スケジュールを、
**それを縛る 2 つの下界**と並べて出します:

```
guff:     seed wave schedule: wall 0.71s vs busy/4 0.46s vs critical path 0.20s
          (busy 1.86s, occupancy 65%); 4/57 waves narrower than 4 threads hold 0.00s
```

- `busy/threads` — 全コアを埋め切り、バリアを全部外した場合の下界。
- `critical path` — 依存チェーンの最長路。**どんなスケジュールでもこれは切れません。**
- `occupancy` — 実測 wall に対する `busy/threads` の比。

**この 2 つを並べずに「バリアが N 秒無駄にしている」と言うのは推測です。**
B-9 を 3 セッションにわたって「原則着手しない」で持ち越せたのは、
誰もこの 2 行を出していなかったからでもあります。

### 2.2 `scripts/perf-profile.py --subtree REGEX`

既存の 2 つは「全体で誰が熱いか」（self）と「X を呼んだのは誰か」（`--callers`）に答えます。
phase を調べるときに要る 3 つ目の質問 —— **「`build_source_seed_inner` の inclusive
1.77s は *何でできているか*」** —— には、どちらも答えられません。
`--inclusive` は合計しか出さず、`--callers` は葉から上に辿るので内訳になりません。

`--subtree` は**マッチしたフレームの下で取られたサンプルを、その葉に付け替えて**集計します。
**パーセンテージは run 全体ではなく部分木に対する比**です（seed の 16.7% が知りたい数字で、
全体の 2.1% ではないため）。

```bash
python3 scripts/perf-profile.py /tmp/guff.json.gz --subtree 'build_source_seed_inner'
python3 scripts/perf-profile.py /tmp/guff.json.gz --subtree 'Action::execute' --top 25
```

**§4.1 の「seed の 16.7% が `__open`」も §3 の `run_resolve` の内訳も、これで出しました。**

---

## 3. V4-1 — コメント再パースで object resolution を切る（**DONE**）

### 何をしたか

`PARSE_COMMENTS` だけで再パースしていた 8 箇所を、新しい共有定数
`guff::parser::COMMENTS_ONLY`（`PARSE_COMMENTS | SKIP_OBJECT_RESOLUTION`）に置き換えました。

| ファイル | 使う人 |
|---|---|
| `guff-comment/src/util.rs` | godot / dupword / godox / godoclint |
| `guff-style/src/gocritic.rs` | gocritic のコメント系チェッカ |
| `guff-style/src/funlen.rs` | funlen |
| `guff-revive/src/util.rs` | revive 6 ルール（package-comments 他） |
| `guff-staticcheck/src/sa1019.rs` | 依存パッケージの `Deprecated:` 走査 |
| `guff-lint/src/nolint.rs` | nolint ディレクティブ索引 |
| `guff-govet/src/buildtag.rs` | build constraint |
| `guff-analysis/src/passes/facts/deprecated.rs` | deprecated fact |

### なぜ効くのか（プロファイルの裏付け）

`parse_file` の inclusive CPU は **3.011s（全体の 21.6%）** で、guff の単一活動として最大です。
そのうち `parser_resolver::run_resolve` が **0.888s**。中身は:

```
0.077s  _platform_memmove
0.069s  guff::scope::Scope::lookup
0.064s  <guff::ast::Expr as Clone>::clone     ← ObjDecl への部分木ディープコピー
0.053s  pthread_mutex_init                    ← Scope 1 個につき Mutex 2 個
0.046s  sip::Hasher::write
0.044s  pthread_mutex_unlock
0.041s  pthread_mutex_lock
0.031s  drop_in_place<guff::ast::Expr>
```

resolution は `Ident.obj` / `file.scope` / `file.unresolved` を埋めるためだけに存在し、
**`Ident.obj` を読む analyzer（ineffassign / maintidx / sloglint / testinggoroutine）は
全員「共有 AST」の方を読みます。再パース結果を読む人は 1 人もいません。**
`file.scope` と `file.unresolved` に至っては `guff-ast` の外で参照ゼロです
（`rg '\.unresolved\b' crates` の結果が `guff-ast/src/resolve.rs` だけ）。

`run_resolve` の呼び元 CPU:

| 呼び元 | CPU |
|---|---:|
| gocritic `run_comment_checks` | 0.267s |
| godot | 0.189s |
| **target typecheck の parse** | 0.176s ← ここは触らない（`skip_object_resolution` が既にある） |
| revive | 0.101s |
| sa1019 `dep_facts` | 0.068s |
| nolint 索引 | 0.018s |

**ルール13（参照実装に既にある最適化を優先する）そのものです。** Go は `go/parser` の
object resolution を既定で切り、機能自体を deprecated にしています。golangci-lint も
staticcheck も `parser.SkipObjectResolution` で読みます。

### 実測

**A/B/A/B 交互、6 往復、`scripts/perf-ab.sh`:**

```
CPU : A median 14.345s  min 14.270  →  B median 13.610s  min 13.540
      delta -0.735s (-5.1%)   min-to-min -0.730     ← 6 往復すべてで B が速い
wall: A median  2.635s  min  2.530  →  B median  2.530s  min  2.460
      delta -0.105s (-4.0%)   min-to-min -0.070
```

**phase 内訳**（`GUFF_DEBUG_CACHE=2`）

| phase | before | after | 差 |
|---|---:|---:|---:|
| load_graph | 0.54s | 0.52s | −0.02 |
| typecheck_roots | 1.10s | 1.08s | −0.02 |
| **analyze** | **0.95s** | **0.84s** | **−0.11（−11.6%）** |
| issues+filter | 0.05s | 0.03s | −0.02 |
| format_checks（並走） | 0.63s | 0.63s | ±0 |
| **peak RSS** | **3.386 GiB** | **3.299 GiB** | **−87 MiB** |

> **RSS が減るのが効きの証拠でもあります。** `ObjDecl` は宣言の部分木を
> **ディープコピーで持つ**ので、resolution を切ると AST 自体が小さくなります。
> V1-4（再パース共有）が **+0.94 GiB** で NO-GO になったのと逆向きです。
> **同じ「再パースが重い」問題に対して、メモリを増やす方向（共有）ではなく
> 減らす方向（仕事を削る）に解があった**、というのが今回の教訓です。

### 検証

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ IDENTICAL (20 issues, order-sensitive) |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL |
| `GUFF_INSPECT_MASKS=0` ≡ 既定 | ✅ IDENTICAL |
| 決定性（同一バイナリ 3 回） | ✅ IDENTICAL |
| `cargo test --release --workspace` | ✅ 3,118 passed / 0 failed |
| `compat/golden/run.sh` | ✅ OK: 81 case(s) match golden exactly |
| `regress --profile full` | ✅ **PASS**（初。§7.2） |
| `regress --profile tsdb` | ⚠️ base と同じ FAIL、RSS は −87 MB（§7.1） |

**`./tsdb/...` でも効きます**（交互 6 往復）: CPU **−4.8%** / wall **−6.5%**。

---

## 4. 測って「やらない」と確定したもの

### 4.1 B-9（seed の wave バリア撤廃）— **最終 NO-GO。理由が変わりました**

V2 §B-9 の理由は「伸び幅 ~0.4s に対して `base_fp` が非決定的になり seed 永続キャッシュを失う」
でした。V3 §4.5 が「analyze と同格になったので再評価の価値はある」と書いたので、測りました。

**まず内訳（`53943d4`, empty cold）:**

```
typecheck_roots 1.10s
  ├─ seed build 0.79s
  │    ├─ wave 並列部  0.71s
  │    └─ merge        0.06s（直列）
  └─ target check 0.28s
```

**merge は 0.06s しかありません。** 直列部分は最初から問題ではありませんでした。

**次にスレッド数を振りました**（`GUFF_RAYON_THREADS`、既定は `(ncpu/2).clamp(3,4)` = **4**）:

| threads | seed wave wall | busy (CPU) | busy/threads | critical path | occupancy | 全体 real | peak RSS |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 4（既定） | 0.71s | 1.86s | 0.46s | 0.20s | 65% | 2.62s | 3.12 GiB |
| 6 | 0.70s | 2.41s | 0.40s | 0.25s | 58% | 2.59s | 3.18 GiB |
| 8 | 0.69s | 2.76s | 0.34s | 0.25s | 50% | 2.62s | 3.22 GiB |
| 10 | 0.67s | **3.08s** | 0.31s | 0.27s | 46% | 2.63s | 3.25 GiB |

**スレッドを 4 → 10 にすると wall は 0.04s しか縮まず、CPU は 1.2s 増えます。**
増分の内訳は `user` +0.83s に対し **`sys` +1.85s**（page reclaims は 223k → 230k で横ばい）。
つまりページフォルトではなく**カーネル側で直列化している**。

**seed の中身を見ると理由が分かります**（`subtree.py 'build_source_seed_inner'`, 1.77s）:

```
0.295s  16.7%  __open      ← 単独 1 位
0.193s  10.9%  Scanner::next
0.150s   8.5%  _platform_memmove
0.119s   6.7%  Scanner::scan
0.066s   3.7%  Parser::parse_body
0.046s   2.6%  blake3_hash_many_neon
```

**seed dep check は 1593 パッケージぶんのソースを開いて読む処理であり、
macOS の `open()` はコアを足しても速くなりません。**

したがって:

- **バリアは制約ではありません。** 完璧なスケジュール（`busy/threads` = 0.46s）に
  到達できたとしても、**その 0.25s は並列化で回収できないことが実測で分かっています**
  （スレッドを 2.5 倍にして 0.04s しか出ない）。
- `base_fp` の設計を変える工数を払っても、**上限が 0.04s** です。ルール14で「やらない」。

**着手してよい唯一の条件を書き換えます:** `seed wave schedule` 行の `occupancy` が
100% に近づき、かつ `critical path` が wall に迫ったとき —— そのときだけスケジュールが
制約になっています。**今は occupancy 65% で、しかもスレッドを足すと下がります。**

> **ついでの発見**: 既定 4 スレッドの上限（`crates/guff-runner/src/lib.rs`）は
> 「for-test-gap seed が型アリーナをほぼ倍にするので RSS ゲートを割る」ため置かれています。
> 上の表のとおり **RSS は 3.12 → 3.25 GiB（+0.13）** で、`peak_rss_ratio: 1.2` は割りません。
> ただし **wall が動かないので上げる理由もありません。** 現状維持が正解です。

### 4.2 O(候補 × AST) はもう残っていません

V3 §4.5 の候補 6 番です。`inspector` を通らない生 walk の呼び元を
プロファイルで全部出しました（`--callers 'walk::preorder::rec|preorder_stack|preorder_prune|walk::walk$'`）:

| CPU | 呼び元 |
|---:|---|
| 0.220s | `revive::rules::shared_walk::run_shared` ← **これは既に「共有」walk** |
| 0.173s | `testifylint::run` |
| 0.103s | `sloglint::run` |
| 0.083s | `govet::testinggoroutine::run` |
| 0.082s | `loggercheck::run` |
| 0.075s | `passes::typeindex::run` |
| 0.057s | `whitespace::run` |
| （以下 0.05s 未満が 10 本ほど） |

**合計しても ~1.0s CPU で、しかも全部「1 analyzer につき AST 1 周」です。**
V1-13（modernize）や V1-3（callcheck）のような **「候補 1 つごとに全走査」の形は
1 つも残っていません**。最大の 0.22s ですら、V1-12 で gocritic を移したのと同じ
「共有 inspector に載せる」話で、**analyze の並列度 9.1× を考えると
全部載せ替えても wall −0.05s 程度**です。ルール14で「やらない」。

> `grep -rn "for file in pass.files()" -A2` は**この形を見つける道具としては役に立ちません**。
> 1 ファイル 1 周の正常な walk が数百件ヒットして、入れ子（候補ループの中の walk）と
> 区別できないからです。V3 §4.5 のこの grep 提案は**取り下げてください**。
> プロファイルの `--callers` で呼び元 CPU を見るのが正解です。

### 4.3 アロケータ ~6% と `Expr::clone` / `drop_in_place<Expr>`

V3 §4.5 の候補 5 番です。**「犯人」ではなく「結果」でした。**

アロケータ合計は自己 CPU で ~0.85s（6.1%）。ただし
`mi_free` / `mi_malloc_aligned` / `mi_page_*` は**何かが割り当てた結果**であって、
そこを直接削ることはできません。削るには割り当てを減らすしかなく、
**V1-11 が「`String` の割当を消しても wall にも CPU にも出ない」を実測済み**です
（mimalloc がほぼ吸収する）。

`drop_in_place<guff::ast::Expr>` 0.221s の呼び元を depth 5 まで出すと、
**最大のチェーンが 0.039s**、以下 0.023 / 0.021 / 0.020 …と散らばります。掴む所がありません。

**ただし 1 本だけ意味がありました**: 最大チェーンが
`drop_in_place<ObjDecl> <- Resolver::declare <- walk_func_decl` で、
これは **V4-1 が消した分**です。つまりこの項目は「アロケータを攻める」のではなく
**「割り当てている仕事そのものを消す」ことで部分的に解決しました**。

`Scope` が **1 個につき `Mutex` 2 個**（`outer` と `objects`）を持ち、
`run_resolve` の 15.5% が `pthread_mutex_init/lock/unlock` だという発見は残っています
（`crates/guff-ast/src/scope.rs:208`）。V4-1 で再パース側は消えましたが、
**target typecheck の parse 0.176s 側にはまだ残っています。** §6 に候補として書きます。

---

## 5. 既存バグ: C-7 seed speculation は 2026-07-31 から発火していません

**未修理です。性能タスクではなく、落ちている機能の修理です。**

`GUFF_DEBUG_CACHE=1` を付けると毎回これが出ます:

```
guff:   seed speculate skip (no golist/stdlib cache peek)
```

**永続 `GUFF_CACHE` の 2 回目・3 回目でも出ます。** つまり
「空 `GUFF_CACHE` だから no-op（設計どおり）」ではありません。

原因は `crates/guff-packages/src/golist.rs:1279`:

```rust
let dir = guff_cache_dir()?;
if !dir.join("golist").is_dir() {
    return None;          // ← ここで必ず抜ける
}
```

C-3c（2026-07-31）で **native lister が既定**になり、`go list` の stdout キャッシュ
（`$GUFF_CACHE/golist/`）を**誰も書かなくなりました**。実際、3 回まわした後の
キャッシュディレクトリの中身は:

```
$ ls $GUFF_CACHE
modmeta  seed
```

`golist/` がありません。**C-7 は「peek 先が存在しない」ので恒久的に no-op です。**

### 直したときの見込み

speculation は seed build を load_graph と重ねる仕掛けなので、上限は
`min(load_graph, seed build)` です。永続キャッシュありの 2 回目で:

| | 1 回目 | 2 回目 | 3 回目 |
|---|---:|---:|---:|
| load_graph | 0.52s | 0.30s | 0.29s |
| seed dep check | 0.77s | 0.32s（1593 hit） | 0.32s |
| typecheck_roots | 1.08s | 0.62s | 0.62s |
| analyze | 0.91s | 0.91s | 0.91s |
| **real** | **2.59s** | **1.91s** | **1.89s** |

→ **上限 ~0.29s / 1.89s（−15%）**。空 cold では 0（peek する物が無い、これは設計どおり）。

**必要な作業**: `peek_cached_graph` に native lister 版の peek を足す。
ただし `--no-cache` では `native_list/` の**書き込み**も止まっているので
（C-3c Phase 3 は peek だけ復活させた）、`modmeta` からグラフを組み直す経路が要ります。
**工数は小さくありません。ユーザー判断を仰いでから着手してください。**

> **教訓**: 「landed した最適化が今も動いている」ことを誰も検査していませんでした。
> §V0-1（per-analyzer 予算のラチェット）と同じ発想で、
> **「speculate が発火した/しなかった」を regress に出す**のが安いと思います。

---

## 6. 次にやる人へ — 第4弾終了時点の地図

V4-1 後、empty cold `./...`:

```
load_graph      0.52s   （CPU 0.99s のうち 72% が open/stat/getdirentries）
typecheck_roots 1.08s   （seed 0.79 + target 0.27。並列化しない = §4.1）
analyze         0.84s   （CPU ~7.3s、並列度 ~8.7）
format_checks   0.63s   （waited=0.00s。**触らない**）
real            2.49s
```

**`__open` は依然としてプロファイル 1 位**（1.07s / 7.7%）で、内訳は
load_graph 0.425s / seed 0.295s / analyze 0.221s。**ただし攻め方は限られます**:

### 着手候補（期待値の高い順）

1. **コメント再パースの「共有」を、RSS を増やさない形で。** V4-1 は 1 回のパースを
   *安く* しましたが、**回数は減っていません**。prometheus の設定では
   gocritic / godot / revive が**同じファイルを 3 回パース**します
   （inclusive: gocritic `run_comment_checks` 0.605s / godot 0.554s / revive 0.213s）。
   V1-4 が NO-GO だったのは「パッケージ全ファイルぶんを analyze フェーズ中ずっと保持」して
   +0.94 GiB 食ったからで、**revive が既に持っている thread-local + パッケージ単位 +
   実行後 `clear_reparse_cache()` の形なら RSS 増はほぼゼロ**のはずです
   （revive が有効な設定では AST は**既に常駐している**ので、共有は増やすのではなく減らす）。
   **上限 ~0.8s CPU ≒ analyze wall −0.09s。** 難所は「revive の実行前後で
   キャッシュの寿命をどう合わせるか」で、そこを外すと V1-4 の再演になります。
2. **`Scope` の `Mutex` 2 個**（`crates/guff-ast/src/scope.rs:208`）。
   `run_resolve` の 15.5% がロック操作でした。V4-1 で再パース側は消えたので、
   残りは target typecheck の parse 0.176s ぶん。**単独では 0.03s 程度**で、
   ルール14では「やらない」側です。**上の 1 番と同時にやるなら測る価値があります。**
3. **C-7 の修理**（§5）。永続キャッシュ 2 回目で上限 −0.29s / −15%。要ユーザー判断。
4. **`sa1019::dep_facts` 0.551s**（analyze CPU の 6.4%）。依存パッケージの
   `Deprecated:` doc をディスクから読み直しています。V4-1 で resolution は消えましたが、
   **read + parse は残っています**。上限 analyze wall −0.06s。

### やってはいけない

- **`format_checks` に手を出さない。** `waited=0.00s`、余裕 ~1.9s。V2 §B-10 の着手条件未達。
- **B-9（wave バリア）に手を出さない。** §4.1 で上限 0.04s を実測済み。
- **`grep "for file in pass.files()"` で O(候補×AST) を探さない。** §4.2。
- **プロファイルの self CPU 順に潰さない。** §1.1（phase で 4 倍違う）と
  §4.3（アロケータは結果であって原因ではない）。

---

## 7. 検証結果（ゲート）

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ **IDENTICAL (20 issues, order-sensitive)**。rebase 後に **`f9af656` のバイナリ相手でも**再確認 |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL |
| `GUFF_INSPECT_MASKS=0` ≡ 既定 | ✅ IDENTICAL |
| 決定性（同一バイナリ 3 回） | ✅ IDENTICAL |
| `cargo test --release --workspace` | ✅ **3,118 passed / 0 failed / 13 ignored**（`f9af656` に rebase 後に再実行） |
| `compat/golden/run.sh` | ✅ **OK: 81 case(s) match golden exactly**（rebase 後に再実行。ratchet は baseline どおり: sa missing 3 / extra 1、st missing 10 / extra 0。PR #6 で linux 限定だった 2 fixture が見えるようになり sa は 257/259 → **260/262**） |
| `regress --profile tsdb` | ⚠️ FAIL（**base も同じ 2 項目で FAIL**。§7.1） |
| `regress --profile full` | ✅ **PASS**（§7.2。**これまで FAIL 続きだったものが初めて通りました**） |

### 7.1 `regress --profile tsdb` は base と同じ FAIL

| | base (`53943d4`) | V4-1 | baseline / limit |
|---|---:|---:|---|
| wall_seconds | 1.020s FAIL | 1.070s / 0.950s FAIL | 0.730 / **0.880** |
| peak_rss_bytes | 1,175,896,064 FAIL | **1,082,097,664** FAIL | 748,388,352 / 898,066,022 |
| guff_only | 0 | 0 | |
| precision / recall | 1.0 / 1.0 | 1.0 / 1.0 | |

**FAIL の中身は base と同一**（baseline が 2026-07-30 の値のまま）。RSS は **−94 MB 改善**。

V4-1 の wall が 2 つあるのは**同じバイナリを 2 回ゲートに掛けた結果**で、
1.070s と 0.950s です。**1 サンプルのばらつきが base との差より大きい**、というのがここの要点。
同じ `./tsdb/...` を交互 6 往復で測ると:

```
CPU : A median 4.670s → B median 4.445s   -0.225 (-4.8%)   min-to-min -0.260
wall: A median 0.995s → B median 0.930s   -0.065 (-6.5%)   min-to-min -0.070
```

**6 往復とも B が速い。** `regress/run.sh` は**各バイナリを 1 回しか測らず、交互でもない**ので、
**2 本のバイナリを比べる道具としては使えません**（V3 §3.2 が `benchmarks/run.sh` について
書いたのと同じ理由）。ゲートとしては正しく、A/B としては使わないでください。

### 7.2 `regress --profile full` が初めて PASS しました

```
| wall_seconds     | baseline 2.360 | measured 2.440 |   limit 2.510  → PASS
| peak_rss_bytes   | 3,114,582,016  | 3,302,457,344  |   limit 3,737,498,419 → PASS
| guff_only 0 / golangci_only 0 / precision 1.0000 / recall 1.0000
## PASS
```

**V3 の記録は 2.930s（超過 0.42s）、その前の main は 3.210s（超過 0.85s）でした。**
V3 が「baseline `2.360s` は 2026-07-30 の値なのでマージ後に `--update-baseline` を回すのが筋」
と書いていた宿題は、**baseline を動かさずに解けました。`--update-baseline` は不要です。**

> **ただし余裕は 0.07s しかありません**（2.440 対 limit 2.510）。負荷の高い日には落ちます。
> 交互 4 往復（`--skip-golangci`、ゲートと同一条件）だと:
>
> | round | base | V4-1 |
> |---|---:|---:|
> | r1 | 2.530s | **2.430s** |
> | r2 | 2.540s | **2.430s** |
> | r3 | 2.540s | **2.470s** |
> | r4 | 2.600s | **2.500s** |
> | 中央値 | 2.540s（**limit 超過**） | **2.450s（PASS）** |
>
> peak RSS も 3.108–3.130 GiB → **3.042–3.079 GiB**。
>
> **注意した方がいい実例**: 交互にする前、1 発ずつ撃ったときは
> **base 2.500s PASS / V4-1 2.680s FAIL** と出ました。**符号が逆に出ています。**
> 交互 4 往復では 4 戦 4 勝で逆転しています。**`regress` の 1 サンプルで
> 良し悪しを判断しないこと。**

---

## 8. 再現コマンド

```bash
# 専用 worktree + 専用 CARGO_TARGET_DIR（他エージェントと target を共有しない）
export CARGO_TARGET_DIR=$HOME/.cargo-targets/guff-perf-v4
cargo build --release -p guff-lint
cargo build --profile profiling -p guff-lint

scripts/perf-guard.sh                       # 通らないなら数字を信じない

# A/B（「仕事を減らしただけ」の変更は cpu が主、wall が従）
scripts/perf-ab.sh /tmp/guff-before target/release/guff --mode cpu  --rounds 6
scripts/perf-ab.sh /tmp/guff-before target/release/guff --mode wall --rounds 6

# phase + seed wave schedule
cd prometheus
C=$(mktemp -d); GUFF_CACHE=$C GOLANGCI_LINT_CACHE=$C GUFF_DEBUG_CACHE=2 \
  /usr/bin/time -l ../target/release/guff run -c .golangci.yml --out-format json \
  --issues-exit-code 0 --no-cache --timeout 15m ./... >/dev/null

# seed のスレッド上限を振る（§4.1 の表）
GUFF_RAYON_THREADS=10 …   # 0 = ncpu

# サブツリー内訳
python3 scripts/perf-profile.py /tmp/guff.json.gz --subtree 'build_source_seed_inner'

# ゲート（**1 サンプルなので A/B には使わない**。§7.1）
./regress/run.sh --profile tsdb
./regress/run.sh --profile full
GUFF_BIN=... ./compat/golden/run.sh
```
