# guff 高速化タスク 第5弾 — 共有キャッシュを「置ける場所」は 1 つしかなかった

> **前提**: `docs/PERF_TASKS.md` §0 の絶対ルール10個、`docs/PERF_TASKS_V2.md` §0 のルール11〜15、
> `docs/PERF_TASKS_V3.md` §3 の検証プロトコルは**すべてそのまま有効**です。再掲しません。
>
> **このファイルの立ち位置**: 第4弾 §6 の「着手候補」を上から順に潰した回です。
> 4 本試して **2 本 landed / 2 本 NO-GO**。効いた 1 本と効かなかった 1 本の違いは
> アルゴリズムではなく**キャッシュの置き場所**でした（§4）。それが分かったので、
> 第1弾から繰り返されてきた「共有すると RSS が増える」問題の**構造的な説明**が付き、
> V1-4（第3弾）の NO-GO 理由の記述が**間違っていた**ことも分かりました（§3.2）。

---

## 📌 セッションを引き継いだ人はここから

> ### 結果（2026-08-15, ブランチ `perf-v5`, ベース `16c8075`）
>
> **landed 2 本 / NO-GO 2 本。**
> - **§2.1 callcheck の 25 重走査を打ち切り**: prometheus `./...` empty-cold の **CPU −1.9%**。
>   findings は **20 件バイト同一**、peak RSS 変化なし。
> - **§2.2 C-7 speculation の修理**: **一度も HIT していなかったのを HIT させました**（§2.2）。
>   ただし**性能タスクではありません**（下記）。
>
> **測って「やらない」と確定したものが 2 本（§3）。同じ穴を掘らないでください。**
> - **§3.1 `code::object_call_name` の memo 化は NO-GO**（CPU **+0.3%**、6 往復すべてで悪化）。
>   V1-11 の結論が別経路で再確認されました。**memo が勝つのは
>   `type_func_name`（受信側の型を描画する重い方）だけ**で、`func_name` は
>   「作り直すほうがハッシュを引くより安い」。
> - **§3.2 コメント再パースの共有は NO-GO**（V4 §6 候補 1）。
>   ただし**理由が今回はっきりしました**。効くけれど RSS を払う、という
>   トレードオフ曲線を実測しました（capacity 512 で CPU −2.3% / RSS **+0.37 GiB**、
>   capacity 128 で CPU −0.6%＝誤差 / RSS +0.09 GiB）。
>
> **V4 §5 の C-7 見積り（−0.29s / −15%）は訂正してください（§2.2）。**
> **warm な実行では `load_graph` が 0.04s しかない**ので、speculation が重ねる相手がありません。
> −15% は `--no-cache` + 永続 `GUFF_CACHE` という**計測専用の組み合わせ限定**の数字で、
> その条件で実測しても **−0.04s（−2.3%）**です。修理する価値はありますが、
> **ユーザーが見る数字は動きません。**
>
> **§4 が今回いちばん重要です。** ランナーは既に
> **「パッケージ単位・全ワーカー共有・最後の消費者が終わったら破棄」というキャッシュ枠**を
> 持っています —— **analyzer の結果**です。そこに `OnceLock` で下げるのが正解で、
> thread-local（§3.2 第1版）でもグローバル（§3.2 第2版）でもありません。
> §2 が効いたのはそこに置いたからで、§3.2 が効かなかったのは置けなかったからです。

**計測日**: 2026-08-15 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（`.golangci.yml`, 118 roots / 1616 pkgs）
**ベース**: `16c8075`（PR #5 / #6 マージ後の main）

---

## 1. 地図 — cold だけを見ていると、CI で起きることが見えない

第4弾までの計測は**すべて空 `GUFF_CACHE` の cold** でした。今回はそれに加えて
**warm（無変更）**と**増分（1 ファイル変更）**を測りました。**増分を測ったのは今回が初めてです。**

`GUFF_DEBUG_CACHE=2`、`prometheus ./...`:

| 場面 | real | load_graph | typecheck_roots | analyze | 備考 |
|---|---:|---:|---:|---:|---|
| 空 cold（`--no-cache`, 毎回 `mktemp -d`） | 2.54s | 0.49s | 1.11s | 0.88s | 第4弾までの計測条件 |
| warm・無変更 | **0.14s** | 0.04s | 0.00s | 0.00s | 118 hits / 0 misses |
| **増分（`model/labels` を 1 行変更）** | **1.94s** | **0.04s** | 0.76s | 0.75s | 64 hits / 54 misses |

**読み方が 3 つ変わります。**

1. **`load_graph` は warm では 0.04s** です。`native_list` キャッシュが効くので、
   cold の 0.49s は「`GUFF_CACHE` が空の初回だけ」の数字です
   （V3 §4.5 が同じことを `go list` について書いていました）。
   **これが §2.2 の C-7 見積り訂正の根拠です。**
2. **増分では seed が 894/1592 ミス**します。1 ファイル変えただけでも、
   `model/labels` のように広く import されるパッケージなら**下流全部の指紋が変わる**ためで、
   これは設計どおり（正しい）です。
3. **増分でも analyze と typecheck がほぼ半々**。つまり
   **cold で効く改善は増分でも効く**（逆も真）。場面ごとに別の最適化は要りません。

---

## 2. landed

### 2.1 V5-1 — callcheck の 25 重走査を「呼んでいる名前の集合」1 つで打ち切る（**DONE**）

#### 何をしたか

`callcheck::run(pass, rules)` は 25 本の staticcheck analyzer から呼ばれ、
**それぞれがパッケージの SSA 命令を全部走査**して、自分の rules に載っている
呼び出し名を探していました。ほとんどのパッケージは**どれにも該当しません**
（SA1030 は `strconv.Quote` 系、SA6000 は `regexp.Match` 系で、普通のパッケージは
どちらも呼ばない）。**該当が無いことを確かめるためだけに、同じ命令列を 25 回舐めていました。**

「このパッケージが呼んでいる名前の集合」は 25 本で共通なので、
**最初に聞いた 1 本が作り、残り 24 本は rules のキーぶんのハッシュ引きで答える**ようにしました。
該当があった analyzer は**従来どおり走査し、従来どおりの順序で報告**します。

置き場所は `BuildIrResult` の `OnceLock`（§4）。既にある `expr_values` /
`src_funcs_all` と同じ形で、25 本とも `requires` に `buildir` を持っているので
**analyzer 側の登録変更はゼロ**です。

#### 実測（`scripts/perf-ab.sh --mode cpu --rounds 8`, 交互）

```
A cpu: median 14.590  min 13.750
B cpu: median 14.320  min 13.530
delta: -0.270 (-1.9%)   min-to-min -0.220      ← 8 往復中 7 往復で B が速い
```

`callcheck::run` の self CPU は 0.315s（analyze CPU の 4.3%）でした。

#### 検証

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ IDENTICAL (20 issues, order-sensitive) |
| peak RSS | 3.038–3.056 GiB → 3.039–3.081 GiB（誤差帯） |

### 2.2 V5-2 — C-7 speculation は一度も HIT していなかった（**DONE / 性能改善ではない**）

#### 何が壊れていたか — バグは 2 つあった

V4 §5 は「`peek_cached_graph` が `$GUFF_CACHE/golist/` しか見ず、native lister 既定化で
そこを誰も書かなくなった。だから毎回 skip する」と書いています。**これは正しい。
ただし 1 つ目のバグでした。**

`native_list/`（native lister が実際に書くほう）を見るようにすると **start はします**。
しかし **MISS します**。しかも `GUFF_NATIVE_LIST=off` で
**C-7 が書かれた当時の `go list` 経路に戻しても MISS します**:

```
guff:   seed speculate start (293 targets, 1792 pkgs)
guff: phase load_graph (go list) 1.06s (118 roots, 1616 total pkgs)
guff:   seed speculate MISS ...
```

**293 対 118。** peek はドライバの生の応答をそのまま使っていましたが、
実際に解析されるグラフは `refine` を通った後のものです
（`P [P.test]` があるとき素の `P` を落とす等で 1792 pkgs / 293 roots → 1616 / 118）。
**したがって C-7 は、どの経路でも、書かれた日から一度も HIT していません。**

#### 直したこと

1. `native_list/` を peek する（1 つ目のバグ）。
2. peek したグラフを `load::peeked_graph_shape` に通す。`connect_imports` と
   dedup を `refine` と**共有**するので、両者が食い違う余地がありません（2 つ目のバグ）。
3. cgo の compiled files を、golist peek と同じキャッシュから attach する。
   これが無いと prometheus の `client_golang/prometheus` が 28 対 30 でズレて、
   **そこ 1 パッケージだけで MISS します**。
4. **MISS が理由を言うようにしました。** 「MISS (fingerprint/targets)」は
   **どちらの入力がどうずれたのかを何も言いません**。今は
   `MISS after 0.16s (github.com/prometheus/client_golang/prometheus: compiled_go_files 28 vs 30)`
   のように**どこがどう違うか**を出します。**上の 2 つのバグはこれで見つけました。**

結果:

```
guff:   seed speculate start (118 targets, 1616 pkgs)
guff:   seed speculate HIT (0.16s wall since start, 118 targets)
```

#### 実測 — V4 §5 の見積りは訂正してください

speculation の上限は `min(load_graph, seed build)` です。§1 の表のとおり
**warm な実行の `load_graph` は 0.04s** なので、上限も 0.04s しかありません。
V4 §5 の表が 0.30s を示していたのは `--no-cache` で回していたからで、
`--no-cache` は issue キャッシュを切るだけでなく **`native_list` の書き込みも止める**ため、
あの条件でだけ load_graph が 0.3s 残ります。

**V4 §5 が想定した条件（永続 `GUFF_CACHE` を温めてから `--no-cache`）で、
毎回キャッシュを作り直して 3 往復**:

| round | base | V5-2 |
|---|---:|---:|
| r1 | 1.70s | **1.65s** |
| r2 | 1.74s | **1.70s** |
| r3 | 1.78s | **1.74s** |

**−0.04s / −2.3%（3 往復とも）。−0.29s / −15% ではありません。**

- **実利用（`guff run`, 永続キャッシュ）**: `use_cache = true` なので
  そもそも speculation は起動しません（`lib.rs` の `if !opts.use_cache && dep_source`）。
  **ユーザーが見る数字は動きません。**
- **`--no-cache` + 空キャッシュ（regress / benchmarks の条件）**: peek 先が無いので no-op。設計どおり。

**それでも入れる理由**: 直す前の状態は「skip」でしたが、1 つ目のバグだけ直すと
**「start して必ず MISS する」＝スレッド 1 本ぶんの seed 構築を毎回捨てる**という、
**skip より悪い状態**になります。中途半端に直すくらいなら触らないほうがよい類のもので、
だから HIT まで持っていきました。

#### 検証

| 検証 | 結果 |
|---|---|
| findings バイト同一（HIT した状態で、並列） | ✅ IDENTICAL (20 issues, order-sensitive) |
| findings バイト同一（HIT した状態で、`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL |
| `peeked_graph_shape` が `refine` と同じ答えを出す | ✅ 単体テスト追加（`load::tests`） |

---

## 3. 測って「やらない」と確定したもの

### 3.1 `code::object_call_name` の memo 化 — **NO-GO（CPU +0.3%）**

V4 §6 には無い項目ですが、増分プロファイルで `object_call_name` が
**self CPU 5 位（0.297s / analyze の 5.0%）**に出ていたので試しました。

`is_call_to` / `is_call_to_any` は 91 箇所から呼ばれ、`CallExpr` を訪れるたびに
`"import/path.Func"` を `format!` で作り直します。V1-5 が SSA 側で同じことを
memo 化して効いた（0.51s → 0.30s）ので、AST 側にも同型の
「pkg id キーの thread-local memo」を入れました。

**結果は 6 往復すべてで悪化（median +0.3%）。** revert しました。

**V1-5 との違いはレンダリングの重さです。**

| 関数 | 中身 | memo |
|---|---|---|
| `type_func_name`（V1-5, SSA 側） | 受信側の型を `type_string` で**描画**する | **効く** |
| `func_name`（今回, AST 側） | arena 参照 2 回 + 短い `format!` | **効かない**（memo のハッシュ引きのほうが高い） |

V1-11（第3弾）が「`String` の割当は mimalloc がほぼ吸収する」と結論した件の
**系**です。**プロファイルで self CPU が見えている ≠ 削れる**の実例をもう 1 つ増やしました。

### 3.2 コメント再パースの共有（V4 §6 候補 1）— **NO-GO。ただし理由が分かりました**

V4 §6 は「revive が既に持っている thread-local + パッケージ単位 + 実行後 clear の形なら
RSS 増はほぼゼロのはず」と書いていました。**そのとおりに作って測ったら +1.0% CPU でした。**

**第1版（thread-local, 1 パッケージ保持）: CPU +1.0% — 一度も当たらない。**

理由はスケジューラです（`crates/guff-runner/src/action.rs`）。ランナーは
**グラフ全体の wavefront** で回します —— 依存が解けた action を**全部 1 つの
`rayon::scope` に spawn** するので、`godot@pkgP` と `gocritic@pkgP` は
**空いているワーカーに散ります**。ワーカーは絶えずパッケージ間を渡り歩くので、
**thread-local はこのスケジューラに対して形が合っていません。**

**第2版（プロセス全体で共有、FIFO で上限を付ける）: 効くが RSS を払う。**

| capacity（ファイル数） | CPU | peak RSS |
|---:|---:|---:|
| 512 | **−2.3%**（6 往復すべてで速い） | 3.05 → **3.42 GiB（+0.37）** |
| 128 | −0.6%（誤差帯） | 3.08 → 3.17 GiB（+0.09） |

**このトレードオフ曲線自体が結論です。** 「共有すれば CPU は減る」は正しく、
**減った CPU のぶんだけ AST を抱える時間が延びる**ので RSS が増えます。
capacity を下げると RSS は戻りますが、同時にヒットも消えます。
`regress` の `peak_rss_ratio: 1.2` を考えると **+12% を −2.3% CPU（wall では ~1%）で
買うのは割に合いません。** revert しました。

> **V1-4（第3弾）の NO-GO 理由の記述は間違っていました。**
> V3 §7.2 は「共有結果は**パッケージ全ファイル分を analyze フェーズの間ずっと保持**する」と
> 書いていますが、ランナーは `release_finished_deps` で
> **消費者が全員読み終えた結果をその場で破棄しています**（`action.rs`）。
> 本当の理由は保持**期間**ではなく保持**時期**で、
> wavefront は「全パッケージの inspect」→「全パッケージの godot」という順に回すので、
> **早いパッケージの結果は最後の消費者が来るまで、フェーズをまたいで生き続けます**。
> V1-4 も今回の第2版も、これに同じだけ課金されていました。

---

## 4. 今回いちばん重要なこと — 共有キャッシュを置ける場所は 1 つ

§2.1 が効いて §3.2 が効かなかった違いは、アルゴリズムではなく**置き場所**でした。
どちらも「パッケージ単位で 1 回だけ計算して全員で使い回す」という同じ形です。

| 置き場所 | パッケージ単位か | 全ワーカーから見えるか | 破棄のタイミング | 実測 |
|---|---|---|---|---|
| thread-local | ✗（ワーカーが渡り歩く） | ✗ | 手動 | **+1.0%** |
| プロセス全体 + FIFO 上限 | ○ | ○ | 上限に押し出されるまで | −2.3% だが **RSS +0.37 GiB** |
| **analyzer 結果の `OnceLock`** | **○** | **○**（`Arc<AnalysisResult>` を全 pass が共有） | **最後の消費者が終わった時点**（`release_finished_deps`） | **−1.9%、RSS 変化なし** |

**ランナーは既に正しいライフタイム管理を持っています。** `pass.result_of::<T>()` は
`Arc` 越しの共有参照で、同じパッケージの全 analyzer が**同一インスタンス**を見ます。
そこに `OnceLock` フィールドを足せば、

- パッケージ単位のキーは不要（インスタンスがパッケージそのもの）
- 排他制御は `OnceLock` が持つ（グローバル `Mutex` を増やさない ← V1-7 の教訓）
- 破棄はランナーがやる（RSS の上限を自分で決めなくていい）

`BuildIrResult` には既に `expr_values` / `src_funcs_all` がこの形で入っていました。
**新しく共有キャッシュを足したくなったら、まず「どの analyzer 結果に下げられるか」を探すこと。**
下げられないなら、それは §3.2 のトレードオフを買うということです。

> **裏取り**（推測で書かないため）: `action.rs` の `ActionState.result` は
> `Option<Arc<AnalysisResult>>` で、依存側には `Arc::clone(result)` で**ポインタだけ**渡ります。
> かつて存在した per-dependent の `clone_result`（型アリーナごとディープコピー）は
> 「wave のワーカーを直列化して +23s の user CPU を焼いた」ため既に外されており、
> コメントがそう書いています。**したがって同一パッケージの全 analyzer は
> 文字通り同じインスタンスを見ます。**

---

## 5. 次にやる人へ

> **2026-08-16 追記: §5.1 は第6弾で着手し、landed しました。
> 先に [`docs/PERF_TASKS_V6.md`](PERF_TASKS_V6.md) を読んでください。**
> 仮説（wavefront が RSS の原因）は当たっていて、**peak RSS −19% / analyze wall −20%**。
> 心配していた「fact の伝播が wave 順に依存していないか」は**依存していません**でした
> （action の依存辺がデータ依存を全部持っているので、任意の topological 実行が同値）。
> **§4 の「共有キャッシュを置ける場所は analyzer 結果 1 つだけ」も、V6 §3.4 で
> 前提が変わったことが確認されています**（置き場所が 1 つだったのは wavefront のせい）。

### 5.1 いちばん大きい未着手 — パッケージ単位スケジューリング（RSS）

§3.2 で分かったこと（wavefront が「全パッケージの analyzer A」→「全パッケージの
analyzer B」の順で回る）は、**RSS そのものの説明にもなっています**。
早いパッケージの `InspectResult` / `BuildIrResult` は、最後の消費者が来るまで生きます。
1616 パッケージぶんが同時に生きうる、ということです（peak RSS 3.0 GiB）。

**パッケージ単位でまとめてスケジュールできれば**、保持は「同時に処理中のパッケージ数」
（≒ワーカー数）に落ちます。RSS のゲート（`peak_rss_ratio: 1.2`）に対する
いちばん大きいレバーはおそらくこれです。

**ただし着手前に**: パッケージ間の fact 伝播が wave 順に依存していないかを必ず確かめること。
依存パッケージの fact が要るので、**パッケージの実行順は依存順のまま**でなければなりません。
「同じパッケージの analyzer をまとめる」だけなら順序制約は変わらないはずですが、
**確かめてから**着手してください（V4 §4.2 の教訓: 「〜だからできない」も「〜だからできる」も仮説）。

### 5.2 やってはいけない（第5弾で追加）

- **`code::object_call_name` を memo 化しない。** §3.1。
- **コメント再パースを共有しない。** §3.2。トレードオフ曲線まで測ってあります。
- **C-7 を「性能改善」として見積もらない。** §2.2。修理は済んでいて、数字は −0.04s です。
- （第4弾から継続）`format_checks` に触らない / B-9 に触らない /
  `grep "for file in pass.files()"` で O(候補×AST) を探さない / self CPU 順に潰さない。

---

## 6. 検証結果（ゲート）

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ IDENTICAL (20 issues, order-sensitive) |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL |
| findings バイト同一（**C-7 が HIT した状態**で） | ✅ IDENTICAL（並列 / `-j 1` とも） |
| `cargo test --release --workspace` | ✅ **3,119 passed / 0 failed / 13 ignored**（V4 の 3,118 + 本 PR の 1 本） |
| `compat/golden/run.sh` | ✅ **OK: 81 case(s) match golden exactly**（ratchet は baseline どおり: sa missing 3 / extra 1、st missing 10 / extra 0） |
| `regress --profile full` | ✅ **PASS**（wall 2.390s / limit 2.510s、peak RSS 3,306,930,176 / limit 3,737,498,419、P=R=1.0000）。ただし 1 サンプルでは落ちもします — 下記 |
| `regress --profile tsdb` | ⚠️ **base と同じ 2 項目で FAIL**（wall / peak RSS）。findings は P=R=1.0000 / `guff_only` 0。RSS は base 1,101,529,088 に対し本ブランチ **1,077,198,848 で低い** |

### `regress --profile full` — 1 サンプルでは符号が反転する

**同じバイナリで 2.390s（PASS）/ 2.630s（FAIL）/ 2.730s（FAIL）が出ます。**
差は**マシンが落ち着いているかどうか**だけで、2.730s は 40 分の compat ゲート直後、
2.390s は load average が 2.5 を割るまで待ってからの値です。
**1 発ずつ撃つと base PASS (2.470s) / 本ブランチ FAIL (2.630s)** とも出ました。
**V4 §7.2 が「まさにこれが起きる」と書いていたもの**です（あちらは逆向きに出ています）。
**交互 4 往復**（`--skip-golangci`、同一条件）だと:

| round | base | perf-v5 |
|---|---:|---:|
| r1 | 2.440s | **2.400s** |
| r2 | 2.440s | **2.420s** |
| r3 | 2.430s | **2.390s** |
| r4 | 2.490s | 2.500s |
| 中央値 | 2.440s | **2.410s** |

**4 戦 3 勝、中央値 −0.03s。両者とも limit 2.510s の下**です。
findings は 20/20 で precision / recall とも 1.0000、peak RSS も
base 3,273,490,432 に対し 3,286,712,320（+0.4%、誤差帯）。

> **`regress` は 1 サンプルなので A/B には使わないこと。** V3 §3.2（`benchmarks/run.sh`）、
> V4 §7.1（`regress` tsdb）、そして今回の full と、**3 回続けて同じ罠にかかっています。**
> ゲートとしては正しく、比較には交互実行を使ってください。

---

## 7. 再現コマンド

```bash
export CARGO_TARGET_DIR=$HOME/.cargo-targets/guff-perf-v5
cargo build --release -p guff-lint
scripts/perf-guard.sh                       # 通らないなら数字を信じない

scripts/perf-ab.sh /tmp/guff-before target/release/guff --mode cpu --rounds 8

# 増分（§1 の 3 行目）を測る: 温めてから 1 ファイル変える
C=$(mktemp -d)
(cd prometheus && GUFF_CACHE=$C GOLANGCI_LINT_CACHE=$C guff run -c .golangci.yml ./... >/dev/null)
printf '\n// perf probe\n' >> prometheus/model/labels/labels_common.go
(cd prometheus && GUFF_CACHE=$C GOLANGCI_LINT_CACHE=$C GUFF_DEBUG_CACHE=2 \
   /usr/bin/time -l guff run -c .golangci.yml --out-format json --issues-exit-code 0 ./... >/dev/null)
git -C prometheus checkout model/labels/labels_common.go
```
