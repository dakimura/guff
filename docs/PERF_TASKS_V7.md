# guff 高速化タスク 第7弾 — RSS の正体は「解放されない」ことだった（macOS）

> **前提**: `docs/PERF_TASKS.md` §0 の絶対ルール10個、`docs/PERF_TASKS_V2.md` §0 のルール11〜15、
> `docs/PERF_TASKS_V3.md` §3 の検証プロトコルは**すべてそのまま有効**です。再掲しません。
>
> **このファイルの立ち位置**: 第6弾 §4 の残タスク（**§4.1 RSS の帰属不明 0.88 GiB** と
> **§4.2 analyze に残っている山**）を全部潰した回です。
> **結論は 2 つとも「もう掘るところが無い」**で、その代わりに
> **掘っている途中で見つかった `-j 1` の取りこぼし**が今回いちばん大きい成果です。

---

## 📌 セッションを引き継いだ人はここから

> ### 結果（2026-08-16, ブランチ `perf-v7`, ベース `2a3d7f7`）
>
> **landed 2 本。**
> - **§2.1 `-j 1` が第6弾の取りこぼしだった**: 逐次パスは wavefront 時代の順序
>   （analyzer 単位）のままで、**全パッケージの結果を最後まで抱えていました**。
>   同じ ready-queue を 1 ワーカーで回すだけで
>   **peak RSS 2959 → 2265 MiB（−23%）/ wall 6.12 → 5.46s（−11%）/ CPU 8.94 → 8.30s。**
> - **§2.2 `GUFF_DEBUG_RSS` が「プロセスの実測 RSS」を出すようになりました**
>   （phase 境界 + seed の 8 wave ごと）。第6弾が estimate しか持っていなかった問題の解消です。
>
> ### いちばん重要な発見 — **macOS では解放しても RSS は下がりません**
>
> `GUFF_DEBUG_RSS=3` で**実行の最後に全部 drop して RSS を測る**プローブを足しました。
> 結果は **0 MiB**。`mi_collect(true)` でも 0 MiB、`MIMALLOC_PURGE_DELAY=0` でも 0 MiB です。
> mimalloc は `MADV_FREE` で purge し、**Darwin はメモリ圧が来るまでそのページを RSS に残す**ためです。
>
> **したがって `/usr/bin/time -l` の `maximum resident set size` は
> 「同時に live だった量」ではなく「commit したページの最高水位」です。** 帰結:
>
> 1. **第6弾 §4.1 の「0.88 GiB は誰のものか」は、drop して測る方法では**このマシンでは**永遠に答えが出ません。**
> 2. **RSS を下げる方法は「同時に生きている量を減らす」しかありません。**
>    第6弾 §2.1（analyze のバリア撤去）が効いたのはまさにそれで、
>    今回の §2.1（`-j 1`）も同じ理由で効きました。
> 3. **アロケータの purge 設定をいじっても無駄**です（§3.1）。
>
> ### 測って「やらない」と確定したものが 4 本（§3）
>
> - **§3.1 `MIMALLOC_PURGE_DELAY=0` にしない。** RSS は 1 MiB も減らず、sys CPU が +1.0s。
> - **§3.2 `is_call_to` から `format!` を消しても効きません。** CPU 中央値の差 **0.000s**。
>   V1-11 → V5 §3.1 → 今回で **同じ結論が 3 回**。**この一族はもう触らないでください。**
> - **§3.3 analyze にはルール14 を超える山がもう残っていません。** 算数は §3.3 に。
> - **§3.4 `rewire_typed_imports` の AST ディープコピーは直す価値がありません。**
>   helm + contextcheck で実測 **0.02s / +44 MiB**（prometheus では**そもそも呼ばれません**）。

**計測日**: 2026-08-16 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（118 roots / 1616 pkgs）、helm（71 roots / 1410 pkgs）
**ベース**: `2a3d7f7`（PR #8 マージ後の main）

---

## 1. 第6弾 §4.1 の宿題 — 帰属不明の 0.88 GiB を追う

### 1.1 まず実測 RSS を phase 境界と seed の途中で出せるようにした

`rss::attribute_packages` は**推定**しか出しません（AST を 1 ノード 192B 固定、
アリーナは slot 分だけ、`Info` はエントリ数×サイズ）。
第6弾はそれで 1.29 GiB を数え、実測 2.17 GiB との差を「不明」と書きました。

`GUFF_DEBUG_RSS` に**プロセスの実 RSS**（`ps -o rss=`）を足しました。出力:

```
guff:   rss now  195 MiB (seed build start)
guff:   rss now  414 MiB (seed wave 40, +77 MiB)
guff:   rss now  941 MiB (seed wave 48, +527 MiB)
guff:   rss now 1072 MiB (seed build done, +1 MiB)
guff:   rss now 2167 MiB (post typecheck_roots, +1096 MiB)
guff:   rss now 2506 MiB (post analyze, +338 MiB)
```

**これで「どの phase が積んだか」が初めて分かります**:
seed が **+877 MiB**、target typecheck が **+1096 MiB**、analyze が **+338 MiB**。
（第6弾 §2.1 の前は analyze が +960 MiB でした。）

### 1.2 次に「解放したら戻るのか」を測った — 戻りません

`GUFF_DEBUG_RSS=3` で、issues を出し切ったあとに**カテゴリ別に drop して RSS を読む**
破壊的プローブを足しました（通常の teardown は `mem::forget`）。

```
guff:   rss now 2526 MiB (teardown start)
guff:   rss now 2526 MiB (after dropping the action graph, +0 MiB)
guff:   rss now 2526 MiB (after dropping syntax + source bytes (118 pkgs, 0 still shared), +0 MiB)
guff:   rss now 2526 MiB (after dropping Info maps, +0 MiB)
guff:   rss now 2526 MiB (after dropping type artifacts (arenas), +0 MiB)
guff:   rss now 2526 MiB (after dropping everything, +0 MiB)
```

**全部捨てても 1 MiB も減りません。** `mi_collect(true)` を挟んでも同じ、
`MIMALLOC_PURGE_DELAY=0` を付けても同じでした。

mimalloc の自己申告（`GUFF_DEBUG_RSS=2` で `mi_stats_print`）:

```
arenas   reserved: 3.0 GiB   committed: 2.3 GiB   purged: 156.9 MiB
pages    abandoned current: 15.7 K
```

**commit 2.3 GiB に対して attribution 1.29 GiB。** 差はフラグメンテーション
（スレッドごとのヒープ、終了したスレッドの abandoned ページ）と、
attribution が下限であること（AST 192B/ノード固定、型の中の `Vec` を数えていない）の合算です。
**そして macOS では、その差が一度でも commit されたら RSS は戻りません。**

> **Linux では違う可能性があります。** プローブは残してあるので、
> `docs/reproduce-guff-linux-ci-in-docker.md` の手順で Linux 上で `GUFF_DEBUG_RSS=3` を回せば、
> **カテゴリ別の実数**が出るはずです。**CI のゲートは Linux なので、そこで測る価値はあります。**

### 1.3 だから RSS の攻め方は 1 つだけ

**「同時に生きている量を減らす」以外に方法がありません。**
第6弾 §2.1 がそれで −19%、今回の §2.1（`-j 1`）が −23%。
**「あとで解放する」「アロケータに返させる」はどちらも効きません。**

---

## 2. landed

### 2.1 V7-1 — `-j 1` は第6弾の取りこぼしだった（**DONE**）

第6弾は `exec_all` の**並列パス**だけを ready-queue にして、
**逐次パス（`-j 1` / ワーカー 1 本）は `order` をそのまま舐める実装のまま**でした。
`order` は topological post-order ですが、root が analyzer 単位で作られるので
**実質 analyzer-major** —— つまり **wavefront と同じ形**です。

同じ ready-queue を **1 本のスタック**で回すようにしました（LIFO なので depth-first、
1 パッケージを終わらせてから次に行く）。cycle 保険も並列パスと同じものを置いています。

**実測（`-j 1` + `RAYON_NUM_THREADS=1`, prometheus `./...`, 2 往復）:**

| | base (`2a3d7f7`) | V7-1 |
|---|---:|---:|
| wall | 6.09 / 6.15s | **5.45 / 5.46s（−11%）** |
| CPU | 8.87 / 9.00s | **8.27 / 8.33s（−7%）** |
| peak RSS | 2959 / 2958 MiB | **2265 / 2265 MiB（−23%）** |

**並列パスは 1 行も変えていません**（A/B で load / typecheck / analyze / wall / RSS すべて誤差内）。

### 2.2 V7-2 — `GUFF_DEBUG_RSS` が実測 RSS を出すようになった（**DONE**）

§1 のとおり。3 段階:

| 変数 | 何が出るか |
|---|---|
| `GUFF_DEBUG_RSS=1` | phase 境界 + seed 8 wave ごとの**実 RSS**、従来の attribution、`mi_collect` 前後 |
| `GUFF_DEBUG_RSS=2` | 上に加えて mimalloc の `mi_stats_print`（reserved / committed / abandoned pages） |
| `GUFF_DEBUG_RSS=3` | 上に加えて**破壊的カテゴリ別 drop プローブ**（teardown を置き換える） |

いずれも**変数を立てないと 1 命令も実行されません**。

---

## 3. 測って「やらない」と確定したもの

### 3.1 `MIMALLOC_PURGE_DELAY=0` — **NO-GO（RSS 不変 / sys +1.0s）**

| | 既定 | `MIMALLOC_PURGE_DELAY=0` |
|---|---:|---:|
| peak RSS | 2495–2533 MiB | 2538–2560 MiB |
| sys CPU | 2.27–2.32s | 3.21–3.25s |
| wall | 2.30–2.59s | 2.46–2.50s |

**RSS は下がらず（むしろ誤差の上側）、sys だけ +1.0s。** §1.2 のとおり Darwin は
`MADV_FREE` したページを回収しないので、purge を急がせても RSS には出ません。

### 3.2 `is_call_to` から `format!` を消す — **NO-GO（CPU 差 0.000s）**

`is_call_to` / `is_call_to_any` は 91 箇所から呼ばれ、`call_name` が
`format!("{path}.{name}")` で作った `String` を**1 回比較して捨てて**います。
そこで**文字列を作らずに比較する**経路（`want` を最後の `.` で割って path / name と直接比較）を
実装しました。V5 §3.1 が試した memo 化と違い、**ハッシュも割当も無い**版です。

**5 往復（順番入れ替え）で CPU 中央値の差 0.000s、min-to-min −0.05s。** revert しました。

> **この一族はこれで 3 回目です**: V1-11（`String` 割当は犯人ではない）→
> V5 §3.1（memo 化は逆に遅い）→ 今回（割当を消しても出ない）。
> **`code::call_name` まわりのアロケーションはもう触らないでください。**
> mimalloc の小サイズ割当は、この規模では計測に出ません。

### 3.3 analyze にはもう山が無い — **算数で確定**

V7 時点の `Action::execute` subtree（samply, 合計 **5.47s** CPU / analyze wall **0.65s** = 8.4×）:

| CPU | symbol |
|---:|---|
| 0.451s | `_platform_memmove`（**結果であって原因ではない**。V4 §4.3 / V6 §4.2） |
| 0.286s | `guff::walk::inspect::rec` |
| 0.257s | `guff::scanner::Scanner::scan`（コメント再パース。共有は V6 §3.4 で NO-GO） |
| 0.207s | `__open` |
| 0.200s | `guff::walk::preorder::rec` |
| 0.142s | `guff_analysis::code::object_call_name`（§3.2） |

**上位 5 個を全部ゼロにしても** 1.40s CPU ÷ 8.4 = **wall −0.17s**、
しかも**どれも実際には消せません**（walk はやる仕事そのもの、memmove は結果）。
**ルール14 の「上限 0.1s」を単独で超える項目は 1 つもありません。**
**analyze は打ち止めです。** 次に触るなら phase 単位ではなくアルゴリズム単位の話になります。

### 3.4 `rewire_typed_imports` の AST ディープコピー — **NO-GO（0.02s / 44 MiB）**

コードを読むと危険に見えます: `imports` を typed パッケージに繋ぎ直すために
**`Package` を丸ごと作り直し、`syntax`（AST 全体）を `clone()`** しています。

**が、`contextcheck` を有効にした設定でしか呼ばれません**
（`analyzers_need_same_module_fact_packages`）。prometheus / helm の**素の設定では 0 回**です。
helm + `contextcheck` だけを有効にして実測すると **0.02s / +44 MiB**（全体 700 MiB）。

**直すには `Package.imports` を内部可変にする必要があり**（Arc グラフなので
`Arc::get_mut` は取れない）、**0.02s のために触る場所ではありません。**

---

## 4. 次にやる人へ

### 4.1 RSS を本気で下げるなら typecheck と analyze の**パイプライン化**

§1.3 のとおり、下げる方法は「同時に生きている量を減らす」だけです。
現状は **全 118 パッケージを型検査し終えてから analyze を始める**ので、
peak には **118 個ぶんの AST（294 MiB 推定）+ `Info`（136 MiB 推定）** が必ず載ります。

パッケージ単位で **型検査 → analyze → syntax/`Info` を捨てる** と流せれば、
そこは「同時実行数ぶん」に落ちます。**第6弾 §2.1 で analyze 側は既にパッケージ単位**なので、
足りないのは typecheck 側との合流だけです。

**着手前に確かめること（全部仮説です）:**
- 依存パッケージの**型情報**は捨てられません（下流の型検査が使う）。捨てられるのは
  `syntax` と `Info` だけで、それが推定 430 MiB です。
- `--fix` は解析後に syntax を使います。`nolint` は**ディスクから読み直している**ので影響しません
  （`nolint.rs` の `fs::read`）。
- `Arc<Package>` は共有されているので、捨てるには**内部可変性**（`OnceLock` / `RwLock`）か、
  `Package` の分割（不変メタ + 可変ペイロード）が要ります。**§3.4 と同じ壁です。**

### 4.2 Linux で `GUFF_DEBUG_RSS=3` を回す

§1.2 のプローブは macOS では 0 しか返しませんが、**Linux（CI のゲート環境）では
カテゴリ別の実数が出るはず**です。ゲートは Linux なので、そこの数字のほうが本番に近い。

### 4.3 やってはいけない（第7弾で追加）

- **アロケータの purge 設定をいじらない。** §3.1。
- **`code::call_name` まわりのアロケーションを触らない。** §3.2。**3 回測りました。**
- **analyze を phase 単位で攻めない。** §3.3。上限が 0.17s で、しかも到達不能。
- **`rewire_typed_imports` を最適化しない。** §3.4。
- （第6弾から継続）seed プールを広げない / 初期 ready を LPT にしない /
  コメント再パースを共有しない / native lister の BFS バリアを外さない。

---

## 5. 検証結果（ゲート）

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ IDENTICAL (20 issues, order-sensitive) |
| 決定性（並列 5 回） | ✅ 5/5 同一 |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ IDENTICAL（**逐次パスを書き換えた回なのでここが本番**） |
| `cargo test --release --workspace` | ✅ **3,119 passed / 0 failed / 13 ignored** |
| `compat/golden/run.sh` | ✅ **OK: 81 case(s) match golden exactly**（ratchet は baseline どおり） |
| `regress --profile full` | ✅ **PASS**（wall **2.240s** / limit 2.510s、peak RSS **2,610,429,952** / limit 3,737,498,419、P=R=1.0000） |
| `regress --profile tsdb` | ⚠️ **base と同じ 2 項目で FAIL**（wall 0.890s / RSS 954,204,160）。第6弾と同じ状況 |

### `regress --profile full` は 1 発目 FAIL、2 発目 PASS でした

**同じバイナリで 2.580s（FAIL）と 2.240s（PASS）。** 差はマシンが落ち着いていたかどうかだけで、
2.580s は 40 分の compat golden 直後、2.240s は load average が 1.5 を割るまで待った値です。
**V3 §3.2 / V4 §7.2 / V5 §6 に続いて 4 回目**なので、もう驚かないでください。
**`regress` は 1 サンプルです。A/B には使えません。**

なお本ラウンドの変更は**逐次パスだけ**で、`regress` は `-j 0`（並列）で回ります。
**つまりこの FAIL は、原理的に本ラウンドの変更とは無関係です。**

---

## 6. 再現コマンド

```bash
export CARGO_TARGET_DIR=$HOME/.cargo-targets/guff-perf-v5
cargo build --release -p guff-lint
scripts/perf-guard.sh

# RSS の内訳（§1.1）
cd prometheus && GUFF_CACHE=$(mktemp -d) GUFF_DEBUG_CACHE=1 GUFF_DEBUG_RSS=1 \
  guff run -c .golangci.yml --no-cache ./... >/dev/null

# 解放しても戻らないことの確認（§1.2）
GUFF_DEBUG_RSS=3 guff run ... 2>&1 | grep 'rss now'

# mimalloc の自己申告（§1.2）
GUFF_DEBUG_RSS=2 guff run ... 2>&1 | grep -A 20 'arenas'

# `-j 1` の A/B（§2.1）
RAYON_NUM_THREADS=1 /usr/bin/time -l guff run -c .golangci.yml --no-cache -j 1 ./...
```
