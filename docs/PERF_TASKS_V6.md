# guff 高速化タスク 第6弾 — バリアを 1 本外したら RSS が 19% 落ちた

> **前提**: `docs/PERF_TASKS.md` §0 の絶対ルール10個、`docs/PERF_TASKS_V2.md` §0 のルール11〜15、
> `docs/PERF_TASKS_V3.md` §3 の検証プロトコルは**すべてそのまま有効**です。再掲しません。
>
> **このファイルの立ち位置**: 第5弾 §5.1 が「いちばん大きい未着手」と名指しした
> **パッケージ単位スケジューリング**をやった回です。仮説は当たっていました
> （RSS −19%）。おまけに **analyze の wall と CPU も落ちました**（キャッシュ局所性）。
> そしてそれが **seed の 4 スレッド上限**という制約を外す余地も作った……ように見えました。
> **測ったら wall −0.06s に CPU +9% で、外しませんでした（§3.1）。**
> 「RSS の予算ができた」は、広げてよい理由にはならないという話です。

---

## 📌 セッションを引き継いだ人はここから

> ### 結果（2026-08-16, ブランチ `perf-v5`（第6弾の作業ブランチ）, ベース `8777fce`）
>
> **prometheus `./...` empty-cold: wall 2.53s → 2.31s（−8.8%）/ CPU 13.40s → 12.19s（−9.1%）/
> peak RSS 3133 MiB → 2525 MiB（−19%）**（順番入れ替え 6 ペアの中央値。**6/6 で V6 が速い**）。
> **増分（warm キャッシュ + 1 ファイル変更）: wall 1.85s → 1.71s（−7.5%）/ peak RSS 2853 → 2408 MiB（−16%）。**
> **`regress --profile full` は wall 2.260s / peak RSS 2,642,460,672 で PASS。**
> **baseline（2.360s / 3,114,582,016）を wall でも RSS でも下回ったのは今回が初めてです。**
> findings は全変更で **20 件バイト同一**（並列 5 回 / `-j 1` + `RAYON_NUM_THREADS=1`）。
>
> **landed 4 本。**
> - **§2.1 analyze の wave バリア撤廃**（本命）。RSS **−0.60 GiB**、analyze wall 0.82 → 0.65s、
>   analyze CPU 7.2 → 6.0s。第5弾 §5.1 の仮説どおりです。
> - **§2.2 govet 3 本（buildtag / directive / inline）が型検査済みのバイトを読み直していた**。
> - **§2.3 SA1019 が依存パッケージのソースを開き直していた**（CPU 0.51 → 0.37s）。
> - **§2.4 native lister の推移的依存計算が直列だった**（0.09 → 0.03s、load_graph 0.52 → 0.47s）。
>   ついでに **`GUFF_DEBUG_CACHE=2` に lister の内訳を出すようにしました**（§2.4）。
>
> **測って「やらない」と確定したものが 3 本（§3）。**
> - **§3.1 seed プールの上限 4 → 6 は入れませんでした。** 一度は入れて、
>   **wall −0.06s に対して CPU +1.1s（+9%）**と分かったので外しました。
>   トレードオフ曲線は §3.1 にあります。**「RSS 予算ができたから広げる」は間違いでした。**
> - **§3.2 初期 ready のパッケージ順を LPT（大きい順）にしない。** wall も RSS も動きません。
> - **§3.4 native lister の BFS バリアは撤廃しない。** level が広い（最大 721）ので
>   バリアは遊んでいません。**これは推測ではなく §2.4 で足した計測行が言っています。**
>
> ### 第5弾からの引き継ぎで**訂正**が 1 つ
>
> V5 §4 は「共有キャッシュを置ける場所は analyzer 結果の `OnceLock` **1 つだけ**」と書きました。
> 正しくは **「置き場所が 1 つしか無かったのは、スケジューラが wavefront だったから」**です。
> wavefront を外した今、パッケージの結果は**そのパッケージが走っている間しか生きない**ので、
> V5 §3.2 が測った「プロセス全体で共有 + FIFO 上限」のトレードオフ曲線も**引き直しになります**
> （あの +0.37 GiB は「1616 パッケージぶんが同時に生きる」前提の値でした）。
> **コメント再パース共有をもう一度測るのは、今なら筋が通ります**（§4.1）。

**計測日**: 2026-08-16 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（`.golangci.yml`, 118 roots / 1616 pkgs）
**ベース**: `8777fce`（第5弾の最後のコミット）

---

## 1. 計測方法の追加 — A/B の順番も入れ替える

第5弾までの `scripts/perf-ab.sh` は **毎ラウンド A → B の順**で回します。
このマシンは連続計測すると**単調に遅くなる**（10 分で wall +10% を観測）ので、
**後に回るほうが常に損をします**。実際 §2 の計測中に
「typecheck が +0.05s 悪化した」ように見えて、**順番を入れ替えたら消えました**。

そこで本ファイルの数字は **A→B と B→A を 1 ラウンドおきに入れ替えて**取っています
（`scratchpad/abba.sh` 相当。ラウンド内のペアで比較する）。
**`perf-ab.sh` にも同じ入れ替えを入れるべきです**（§4.4）。

**ドリフト自体は消せません**。1 ラウンド目の絶対値（wall 2.45s）と 6 ラウンド目（2.70s）は
同じバイナリで 10% 違います。**ペア内の差だけを読んでください。**

### 1.1 4 つの場面すべてで測りました

第5弾 §1 が「増分を測ったのは今回が初めて」と書いたので、今回は**空 cold / 増分 / warm /
`-j 1` の 4 つ**を base と本ブランチで取りました（すべて順番入れ替え）。

| 場面 | base (`8777fce`) | perf-v6 | 差（ペア内中央値） |
|---|---:|---:|---|
| **空 cold** wall | 2.45–2.63s（中央 2.53） | **2.21–2.37s（中央 2.31）** | **−0.245s / −8.8%**（6/6） |
| 　　　　　peak RSS | 3118–3158 MiB（中央 3133） | **2514–2560 MiB（中央 2525）** | **−608 MiB / −19%** |
| 　　　　　CPU（user+sys） | 13.40s | **12.19s** | **−1.21s / −9.1%**（4/4） |
| 　　　　　load_graph | 0.52–0.56s | **0.45–0.49s** | −0.06s |
| 　　　　　typecheck_roots | 1.06–1.17s | 1.06–1.18s | 変化なし |
| 　　　　　analyze | 0.81–0.83s | **0.64–0.67s** | −0.16s |
| **増分**（1 ファイル変更） wall | 1.85–1.87s | **1.69–1.74s** | **−0.14s / −7.5%** |
| 　　　　　peak RSS | 2837–2879 MiB | **2396–2423 MiB** | **−445 MiB / −16%** |
| 　　　　　analyze | 0.70–0.71s | **0.56–0.60s** | −0.13s |
| **warm**（無変更） wall / RSS | 0.13s / 127 MiB | 0.13s / 128 MiB | 変化なし（解析ゼロなので当然） |
| **`-j 1`**（+`RAYON_NUM_THREADS=1`） wall / RSS | 6.12–6.13s / 2950 MiB | 6.12s / 2955 MiB | 変化なし（逐次経路は未変更） |

**typecheck が動かないのが正しい姿です。** 途中で seed のプール幅を広げて
typecheck を −0.06s にした版がありましたが、CPU を +1.1s 払うと分かって外しました（§3.1）。

---

## 2. landed

### 2.1 V6-1 — analyze の wave バリアを外す（**DONE / 本命**）

#### 何が起きていたか

`exec_all` は action DAG を**レベル（wave）ごとに `rayon::scope` で回して**いました。
つまり **全 118 パッケージの `inspect` → 全 118 の `buildir` → 全 118 の `gocritic`** …の順です。

producer の結果は**最後の consumer が読み終わるまで捨てられません**
（`release_finished_deps`）。バリアがあると、その consumer は**次の wave**、
すなわち**他の全パッケージが追いつくまで来ません**。
結果として analyze の峰では **118 パッケージぶんの `InspectResult` /
`BuildIrResult` / `Index` が同時に生きて**いました。

**`GUFF_DEBUG_CACHE=2` の per-package 表が 1 列で白状していました**:

```
1.52s CPU  206 actions  [  0.00s..  0.81s]  .../tsdb
0.35s CPU  202 actions  [  0.00s..  0.79s]  .../storage/remote
0.34s CPU  201 actions  [  0.00s..  0.79s]  .../scrape
       …118 パッケージ全部が [0.00s..0.79s]（phase は 0.81s）
```

#### 何をしたか

**依存が全部終わった瞬間に、終わらせたワーカーが spawn する**方式に変えました
（ready-queue / 非同期 DAG）。バリアはありません。

- `indeg[i]` = まだ終わっていない依存の数。0 にしたワーカーが spawn する担当。
- 初期 ready（依存ゼロの action）は**パッケージ単位でまとめて**並べる。
  rayon は steal するワーカーに**連続したタスク**を渡すので、まとめないと
  10 ワーカーが 10 パッケージに散ります。
- **cycle 保険**: scope を抜けたあと `indeg != 0` の action が残っていたら直列で実行します。
  `validate` と import グラフの非循環性で起きないはずですが、
  **「起きたときに findings が黙って減る」のはこのスケジューラが絶対にやってはいけない失敗**なので。

#### 実測（順番入れ替え 6 ラウンド、ペア内比較）

| | base | V6-1 |
|---|---:|---:|
| analyze phase | 0.81–0.83s | **0.64–0.67s** |
| analyze CPU（per-package 表の合計） | 7.24s | **5.95s** |
| analyze CPU（samply, `Action::execute` subtree） | 6.99s | **5.69s** |
| peak RSS | 3118–3158 MiB | **2514–2560 MiB** |
| wall | 2.45–2.63s | **2.21–2.37s** |

**RSS は −0.60 GiB（−19%）。** RSS のタイムラインを 20ms 間隔で取ると、
base は typecheck 終了時 2170 MiB → analyze で 3130 MiB まで**単調に増えて**いました
（**analyze だけで +960 MiB**）。V6-1 後は同じ区間が **2192 → 2575 MiB（+383 MiB）**です。

> **その結果、RSS の主役が移りました。** peak 2575 MiB のうち **2192 MiB は
> typecheck を終えた時点で既に確保されています**。**次に RSS を攻めるなら analyze ではなく
> seed/typecheck です**（§4.2）。

**CPU も −1.3s 落ちます。** 仕事量は同じなので、これはキャッシュ局所性です
（1 パッケージの結果を、そのパッケージの全 analyzer が続けて読む）。

per-package 表も形が変わります:

```
1.48s CPU  206 actions  [  0.35s..  0.63s]  .../tsdb
0.33s CPU  205 actions  [  0.00s..  0.34s]  .../web/api/v1
0.25s CPU  199 actions  [  0.00s..  0.15s]  .../discovery/aws
```

#### 検証

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ IDENTICAL (20 issues, order-sensitive) |
| 決定性（並列 5 回） | ✅ 5/5 同一 |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL |
| `-j 1` + `RAYON_NUM_THREADS=1` の wall / RSS | 6.13s / 2950 MiB → 6.00s / 2962 MiB |

### 2.2 V6-2 — govet 3 本が、型検査が読んだバイトを読み直していた（**DONE**）

`buildtag` / `directive` / `inline` は**パッケージの全ファイルを `fs::read` し直して**、
1 行のバイトゲート（`+build` / `//go:` / `go:fix inline`）で**ほぼ全部を捨てて**いました。
バイトは既にメモリにあります —— typecheck が `Package::source_files` に持っていて、
`revive` / `godot` / `gocritic` / `gosec` は**何か月も前から `source_bytes(i)` で読んでいます**。
この 3 本が移行し忘れていただけでした。

ゲート自体も `windows(n).any(...)`（1 バイトずつの走査）だったので `memchr::memmem` にしました。
同じ最初の一致を返すので判定は 1 ビットも変わりません。

**`inline` の合計 CPU 0.11 → 0.08s。** `buildtag` / `directive` は per-analyzer 表の
top 20 に入らないので単独では測れていません（合計 `__open` は §2.3 と合わせて効きます）。

### 2.3 V6-3 — SA1019 が依存パッケージのソースを開き直していた（**DONE**）

samply で SA1019 の 0.58s CPU を割ると **`__open` が 0.197s（34%）**でした。
`Deprecated:` を探すために、依存パッケージのファイルをディスクから読んでいます。

**同じモジュール内の依存は、たいてい guff がさっき型検査した root パッケージそのもの**で、
そのバイトは `source_files` に残っています。そこで
`imp.source_bytes(idx)` を優先し、無いとき（module cache / GOROOT / export data）だけ読みます。
`prefer_package_doc_files` が index を返すようにして、`source_files` の添字と対応させました。

バイトプローブ（`src_has_deprecated_doc` / `src_has_package_deprecated_doc`）も
`windows().any()` → `memmem` にしました。これが残り 1/4 です。

**SA1019 の合計 CPU 0.51 → 0.37s（−27.5%、5 ラウンド中 5 回）。**

### 2.4 V6-4 — native lister の推移的依存計算が直列だった（**DONE**）

`load_graph` は cold 0.53s ですが、**native lister の中を誰も測っていませんでした**。
まず `GUFF_DEBUG_CACHE=2` に内訳を出すようにしました:

```
guff:     native list bfs: 10 levels (widest 721, 2 narrower than 4 holding 0.00s);
          scan 0.22s parallel + fan-out 0.04s serial
guff:     native list post: fortest 0.02s + transitive deps 0.09s + sort/flush 0.02s
guff:   native list 0.41s (1792 pkgs)
```

**`deps` を埋める 0.09s が、この phase に残っていた最大の直列区間**でした
（1792 パッケージぶんの推移的閉包を 1 スレッドで順に計算）。
各パッケージは `direct_imports` を読むだけで書かないので `par_iter` にできます。

**推移的依存 0.09 → 0.03s / native list 0.41 → 0.35s / load_graph 0.52 → 0.47s。**

`GUFF_NATIVE_LIST=verify` は **OK (1792 pkgs)** のまま（`deps` は `go list` と一致）。

---

## 3. 測って「やらない」と確定したもの

### 3.1 seed プールの上限 4 → 6 — **NO-GO（wall −0.06s に CPU +1.1s は高すぎる）**

`init_rayon_global_stack` は seed/target typecheck のプールを **4 に絞って**います。
理由は RSS で、for-test seed が型アリーナをほぼ倍にするため、ncpu 個のワーカーが重なると
regress の RSS 上限を割る、というものでした。**§2.1 が 0.6 GiB 返したので、
「予算ができたから広げられる」と考えて実際に広げ、6/6 ペアで wall −0.06s を確認しました。**

**それから CPU を見ました。**（同一バイナリ、`GUFF_RAYON_THREADS` だけ変えて 4 ペア）

| | 4 workers | 6 workers | 差 |
|---|---:|---:|---|
| typecheck_roots | 1.07–1.10s | **1.02–1.04s** | −0.06s |
| wall | 2.22–2.26s | **2.15–2.20s** | −0.06s |
| user CPU | 9.92–10.07s | 10.34–10.46s | **+0.42s** |
| sys CPU | 2.20–2.27s | 2.90–2.93s | **+0.68s** |
| CPU 合計 | 12.15–12.34s | 13.24–13.39s | **+1.10s（+9%）** |
| peak RSS | 2509–2555 MiB | 2543–2607 MiB | +40 MiB |

**wall を 2.6% 縮めるために仕事を 9% 増やす取引**です。しかも増分の 6 割は **sys**
（ワーカーが増えたぶんのページフォルトと syscall）で、**性能コア 4 のマシンで
6 本目・5 本目は効率コアに載ります**。CI のように他のジョブと同居する環境では
この 1.1s は他人から取った時間になります。**入れませんでした。**

**8 も測ってあります: wall は 6 と同着、RSS だけさらに +110 MiB**（§3.3）。

> **教訓**: RSS の予算ができたことは、**広げてよい理由にはなりません**。
> 上限が RSS で決まっていたからといって、外したときに払うのが RSS だけとは限らない。
> **`--mode cpu` を必ず一緒に見ること。**

### 3.2 初期 ready のパッケージ順を LPT にする — **NO-GO（差なし）**

§2.1 のあと、per-package 表の tail は **tsdb（1.48s CPU / 全体の 25%）が
0.35s に始まって 0.63s に終わる**＝最後でした。「重いパッケージから始めれば
phase が早く終わる」（LPT スケジューリング）は自然な次の一手です。

`source_files` の合計バイトを重さの proxy にして降順に並べ替えました。
**analyze wall は 0.66–0.68s で変わらず、RSS も変わらず、CPU はむしろ +0.3s。**
5 ラウンドとも差が出ませんでした。revert 済みです。

**なぜ効かないか**: tsdb は 206 action あって**それ自体が内部で並列**なので、
「最後に始まる」ことと「最後に終わる」ことの間に 0.28s しかありません。
一方で LPT にすると小さいパッケージが後ろに溜まり、そちらの tail が伸びます。

### 3.3 seed プールを 8 にする — **NO-GO（6 と同着 / RSS だけ +110 MiB）**

§3.1 を測る過程で 8 も測りました: **typecheck も wall も 6 と同着で、peak RSS だけ
+110 MiB**（4 workers 比）。10 コア中 性能コアが 4 なので、6 を超えると
効率コアに載って 1 ワーカーあたりが遅くなり、合計は変わりません。
**§3.1 で 6 自体を入れないことにしたので、この行は「もし将来広げたくなったら
8 ではなく 6 まで」という記録です。**

### 3.4 native lister の BFS バリアを撤廃する — **NO-GO（バリアが遊んでいない）**

§2.4 で足した計測行が答えです: **10 levels、最大幅 721、
プールより狭い level は 2 本で合計 0.00s。** つまり
「level の切れ目でワーカーが遊ぶ」現象は起きていません。
`GUFF_RAYON_THREADS` を 4 → 8 にしても **load_graph は 0.55s で動きません**でした。

BFS の scan 0.22s は**純粋に syscall 待ち**（`__open` が phase CPU の 51%）で、
残る手は「開くファイルを減らす」しかありません。**そして warm では
`native_list/` キャッシュが効いて load_graph は 0.04s** なので、
ここを詰めても**効くのは「そのマシンで初めて走った 1 回」だけ**です。

---

## 4. 次にやる人へ

### 4.1 いちばん筋が良い未着手 — コメント再パース共有の**再測定**

**V5 §3.2 の NO-GO は「wavefront 前提」の測定でした。**
あのときの「プロセス全体で共有、capacity 512 で RSS +0.37 GiB」は、
**1616 パッケージぶんの AST が同時に生きうる**スケジューラでの値です。
§2.1 でパッケージの寿命は「そのパッケージが走っている間」に縮みました。

いま同じことをやると、共有キャッシュに載るのは**同時に処理中のパッケージ
（≒ワーカー数）ぶん**のはずです。**capacity 32〜64 で当たるかどうかを測るのが次の一手**で、
当たるなら V5 が測った CPU −2.3% を RSS ほぼ据え置きで取れます。

**ただし**: V5 §3.2 の第1版（thread-local）が当たらなかった理由は
「ワーカーがパッケージ間を渡り歩く」ことでした。§2.1 の depth-first は
**渡り歩きを減らしただけで、無くしてはいません**（steal は起きる）。
**thread-local を再挑戦する前に、1 パッケージの action 群が同じワーカーに
どれくらい留まるかを測ってください。**

### 4.2 いま RSS を攻めるなら typecheck（analyze ではない）

§2.1 の RSS タイムラインが言うとおり、**peak 2575 MiB のうち 2192 MiB は
typecheck を終えた時点のもの**です。内訳（`GUFF_DEBUG_RSS=1`, 変化なし）:

```
type arenas: types=545.6MiB objects=230.0MiB scopes=35.5MiB names=8.5MiB intern=23.5MiB (計 843.4MiB)
Info maps:   136.2MiB
AST est:     294.2MiB envelope (1,606,518 nodes × ~192B)
source bytes: 11.0MiB (671 files, Arc-deduped)
attributed:  1290.9MiB（下限。SSA IR・アロケータメタデータ・スタックを含まない）
```

**attributed 1.29 GiB に対して実測 2.2 GiB** なので、**まだ 0.9 GiB が誰のものか分かっていません**。
seed のマージ中に出る一時オブジェクトか、mimalloc が OS に返していないぶんです。
**次に RSS をやるなら、まずこの差を埋めること**（`GUFF_DEBUG_RSS` を seed の途中でも取れるようにする）。
analyze 側は §2.1 で +383 MiB まで落ちているので、**残っている伸びしろは小さい**です。

### 4.3 analyze に残っている山（プロファイルは取り直してあります）

V6 後の `Action::execute` subtree（samply, 合計 **5.69s** CPU。V6 前は 6.99s）:

| CPU | symbol |
|---:|---|
| 0.487s | `guff::walk::inspect::rec` |
| 0.433s | `_platform_memmove` |
| 0.188s | `guff::walk::preorder::rec` |
| 0.182s | `guff_analysis::code::object_call_name` |
| 0.151s | `__open`（V6 前は 0.305s。§2.2 / §2.3 で半減） |

**`memmove` は追いかけないでください。** 呼び出し元を depth 2 で割ると
最大が `RawVec::finish_grow` 0.111s、次が `String as fmt::Write` 0.103s と**散っています**
（V4 §4.3 と同じ結論。あちらは `Expr::clone`/`drop` で同じことを確かめました）。

`object_call_name` は V5 §3.1 で memo 化が NO-GO です。**キャッシュ局所性が変わったので
再測定する価値はありますが、同じ結論が出たらそれで 2 回目です。二度と触らないこと。**

### 4.4 `scripts/perf-ab.sh` に順番入れ替えを入れる

§1 のとおり、A→B 固定だと**熱ドリフトが常に B に課金されます**。
本ファイルの計測はスクラッチのスクリプトでやりましたが、
**ハーネス本体に入れるべきです**（`--swap` ではなく既定で）。

### 4.5 やってはいけない（第6弾で追加）

- **初期 ready を LPT で並べ替えない。** §3.2。
- **seed プールを広げない（4 のまま）。** §3.1（6 でも CPU +9%）/ §3.3（8 は 6 と同着）。
- **native lister の BFS バリアを外さない。** §3.4。計測行が既にあります。
- （第5弾から継続）`code::object_call_name` を memo 化しない（ただし §4.3 の注記あり）/
  C-7 を性能改善として見積もらない。
- （第4弾から継続）`format_checks` に触らない / B-9 に触らない /
  `grep "for file in pass.files()"` で O(候補×AST) を探さない / self CPU 順に潰さない。

---

## 5. 検証結果（ゲート）

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ IDENTICAL (20 issues, order-sensitive)。**5 本の変更それぞれで確認** |
| 決定性（並列 5 回） | ✅ 5/5 同一 |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL |
| `cargo test --release --workspace` | ✅ **3,119 passed / 0 failed / 13 ignored**（第5弾と同数） |
| `compat/golden/run.sh` | ✅ **OK: 81 case(s) match golden exactly**（ratchet は baseline どおり: revive missing 1 / extra 4、qf 1/0、s 2/0、sa 3/1、st 10/0） |
| `GUFF_NATIVE_LIST=verify` | ✅ **OK (1792 pkgs)**（§2.4 が `deps` の作り方を変えたので必須） |
| `regress --profile full` | ✅ **PASS**（wall **2.260s** / limit 2.510s、peak RSS **2,642,460,672** / limit 3,737,498,419、P=R=1.0000） |
| `regress --profile tsdb` | ⚠️ **base と同じ 2 項目で FAIL**（wall / peak RSS）。**ただし base より良い**（下記） |

### `regress --profile full` — baseline を初めて両方で下回りました

| | base (`8777fce`) | perf-v6 | baseline / limit |
|---|---:|---:|---:|
| wall | 2.450s | **2.260s** | 2.360s / 2.510s |
| peak RSS | 3,288,760,320 | **2,642,460,672** | 3,114,582,016 / 3,737,498,419 |
| precision / recall | 1.0000 / 1.0000 | 1.0000 / 1.0000 | — |

第5弾は「limit 2.510s に対して 2.390〜2.730s で、1 サンプルでは符号が反転する」状態でした。
**今回は 2.260s で、limit まで 0.25s の余裕があります**（第5弾は 0.12s）。
**baseline 更新は要りません**（§0-6 のとおり、ユーザー承認が要る作業でもあります）。

### `regress --profile tsdb` — FAIL は base から継続、数字は改善

| | base (`8777fce`) | perf-v6 | baseline / limit |
|---|---:|---:|---:|
| wall | 0.960s | **0.890s** | 0.730s / 0.880s |
| peak RSS | 1,088,995,328 | **959,053,824** | 748,388,352 / 898,066,022 |
| guff_only / golangci_only | 0 / 0 | 0 / 0 | 0 / 0 |

**同じ 2 項目で base も FAIL します**（V3 §3 の検証プロトコル 4 番のとおり確認済み）。
baseline は 2026-07-30 のもので、tsdb だけこの数か月ぶんの回帰を吸収できていません。
**本ブランチは base に対して wall −0.07s / RSS −130 MB** なので、悪化の持ち込みはありません。
wall は limit 0.880s に対して 0.890s ——**あと 10ms** です。

---

## 6. 再現コマンド

```bash
export CARGO_TARGET_DIR=$HOME/.cargo-targets/guff-perf-v5
cargo build --release -p guff-lint
scripts/perf-guard.sh                       # 通らないなら数字を信じない

# 順番入れ替え A/B（§1）。perf-ab.sh は本ラウンドで入れ替え対応にしました
scripts/perf-ab.sh /tmp/guff-before target/release/guff --mode cpu --rounds 4
scripts/perf-ab.sh /tmp/guff-before target/release/guff --rounds 6      # wall

# phase 別 + peak RSS を 1 行で見るときは GUFF_DEBUG_CACHE=1 + /usr/bin/time -l:
#   awk '/phase load_graph/{...} /phase analyze/{a=$5} / real /{w=$1} /maximum resident/{r=$1}'
# RSS のタイムライン（§2.1 の「単調に増える」を見るため）は
#   20ms ごとに ps -o rss= を取り、phase 行と突き合わせる

# 増分（V5 §1 の 3 行目）
C=$(mktemp -d)
(cd prometheus && GUFF_CACHE=$C GOLANGCI_LINT_CACHE=$C guff run -c .golangci.yml ./... >/dev/null)
printf '\n// perf probe\n' >> prometheus/model/labels/labels_common.go
(cd prometheus && GUFF_CACHE=$C GOLANGCI_LINT_CACHE=$C GUFF_DEBUG_CACHE=2 \
   /usr/bin/time -l guff run -c .golangci.yml --out-format json --issues-exit-code 0 ./... >/dev/null)
git -C prometheus checkout model/labels/labels_common.go

# native lister の内訳（§2.4 で追加）
GUFF_DEBUG_CACHE=2 guff run ... 2>&1 | grep 'native list'
```
