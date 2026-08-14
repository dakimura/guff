# guff 高速化タスク 第3弾 — 「バグ修正で積み上がった analyze 費用」を構造的に落とす

> **前提**: `docs/PERF_TASKS.md`（第1弾）§0 の絶対ルール10個、`docs/PERF_TASKS_V2.md`（第2弾）
> §0 の追加ルール（11〜15）、§1.2 の計測ガード、§2 の検証プロトコルは**すべてそのまま有効**です。
> このファイルはそれらを再掲せず参照します。着手前に V2 §0 と §2 を読むこと。
>
> **このファイルの立ち位置**: 第1弾・第2弾は「起動 → `go list` → typecheck → format」という
> **パイプラインの外側**を削り切りました。その結果 cold の重心が **analyze フェーズに移動**し、
> かつ compat hardening（2026-08 以降のバグ修正ラッシュ）で analyze が**再び太り続けています**。
> 第3弾は **analyze の中身**を対象にします。

---

## 📌 セッションを引き継いだ人はここから

> ### 結果（2026-08-14, ブランチ `perf-v3`）
>
> **`b5dbcb8`（rebase 後の main）比: prometheus `./...` cold 3.445s → 2.745s（−20.3%）。**
> **analyze phase 1.59s → 0.95s（−40%）。inspector 走査 198.2M → 11.1M（−94.4%）。**
> RSS は 3.21 GiB → 3.19 GiB でわずかに低下。findings は **20 件でバイト同一**。
>
> 分岐元の `953d243` 比では −35% でしたが、その後 main が**同じ regex 修正を独立に入れた**
> （§7.1.8）ぶん差が縮んでいます。**下の §7.1.5 の表は `953d243` 比の記録**で、
> 現在の main 比は上の数字です。
> **peak RSS 変化なし。findings は HEAD とバイト同一**（並列 / `-j 1` / masks on / masks off）。
> `cargo test --workspace` 3,116 passed、`compat/golden` 81 ケース全一致。
> 詳細は [§7](#7-進捗)、次にやることは [§4.5](#45-次にやる人へ--v1-完了後の地図2026-08-14)。
>
> 一番効いたのは**計画に無かった項目**でした（§7.1）。
> V1-1〜V1-3 を入れてプロファイルを取り直したら、それまで見えていなかった
> `FileSet` の**グローバル `Mutex` が 1 位（2.9s / 14.3%）**に浮上しています。
> **1 つ潰すたびに測り直すこと。**

**計測日**: 2026-08-14 / Darwin 25.2.0 arm64（Apple M4, 10 core, 24 GiB） / go1.26.4
**対象**: `prometheus ./...`（`.golangci.yml`, 118 roots / 1616 pkgs）, cold, `--no-cache`
**着手時のベース**: `953d243`

- **regress full ゲートは FAIL 中**: wall **3.16s** vs baseline **2.36s**（+34%）。
  `regress/results/RESULTS.full.md` に記録済み。tsdb プロファイルは 0.77s / 0.73s で PASS。
- **cold の直列 phase 内訳（`GUFF_DEBUG_CACHE=2`, 3 サンプル中央値）**

  | phase | wall | V2 §1.3-post2 (2026-07-30) | 差 |
  |---|---:|---:|---|
  | startup | 0.00s | 0.01s | — |
  | load_graph（native list） | 0.60s | 0.85s | **−0.25** |
  | cache setup+partition | 0.00s | 0.00s | — |
  | typecheck_roots | 1.23s | 1.45s | **−0.22** |
  | **analyze** | **1.89s** | **0.37s** | **+1.52** ← 全部ここ |
  | issues+filter | 0.03s | 0.03s | — |
  | format_checks | 0.74s（内側に隠れて `waited=0.00s`） | 1.65〜1.81s | −0.9 |

  直列合計 3.75s ＝ `real` 3.78s（`GUFF_DEBUG_CACHE=2` の計装込み）。**未計測区間はない。**

- **`analyze` は今や最大の phase であり、第1弾・第2弾が一度も手を入れていない領域です。**

---

## 1. 何が analyze を太らせているのか（推測ではなく実測）

### 1.1 preorder は 97% が捨てられている

```
guff: inspect preorder: 16452 calls, 202,742,238 nodes scanned, 7,012,186 delivered
      (97% filtered by mask), 3.12s total CPU (21.5% of analyze CPU)
```

`InspectResult::preorder_typed` は**フラット化済みイベント配列を毎回頭から線形走査し、
マスクに合わない 97% をその場で捨てています**。呼び出し側のマスク幅を数えると:

| マスク幅 | 呼び出し箇所 |
|---|---:|
| 1 kind | **119** |
| 2 kinds | 15 |
| 3 kinds | 3 |
| 非リテラル（動的） | 11 |

つまり **ほぼ全員が「1 種類だけ欲しい」と言っているのに、毎回全ノードを見ています**。
per-analyzer の内訳では「1,497,576 scanned → 104,896 delivered」（CallExpr のみ、7% ヒット）という
行が数十本並びます。

### 1.2 `TypeArena` を**呼び出しごとに丸ごと clone** している箇所が約 30 本

samply（`target/profiling`, 総 CPU 20.07s）の self 上位:

```
1.468s  7.32%  guff::walk::preorder::rec
1.367s  6.81%  _platform_memmove
1.362s  6.79%  <guff_types::arena::TypeArena as Clone>::clone
0.969s  4.83%  __open
0.593s  2.95%  read
0.520s  2.59%  guff::scanner::Scanner::scan
0.513s  2.56%  mi_free
0.497s  2.47%  guff::walk::inspect::rec
0.467s  2.33%  guff::scanner::Scanner::next
0.359s  1.79%  mi_malloc_aligned
0.354s  1.76%  guff_analysis::code::object_call_name
0.335s  1.67%  InspectResult::visit_masked
0.334s  1.66%  drop_in_place<guff::ast::Expr>
0.255s  1.27%  drop_in_place<Vec<guff_types::arena::TypeData>>
0.231s  1.15%  core::hash::BuildHasher::hash_one
0.182s  0.91%  core::hash::sip::Hasher::write
```

`Action::execute` の inclusive は **12.98s / 20.07s（64.7%）** ＝ analyze フェーズ。

原因は API 形状です。`identical` / `implements` / `lookup_field_or_method` は
**インターンで arena が伸びうるので `&mut TypeArena` を取る**。呼び出し側はパッケージの
arena を壊せないので、**毎回まるごと clone してから渡しています**:

```rust
// crates/guff-style/src/unconvert.rs:48
fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let mut types = artifacts.types.clone();   // ← 変換式 1 個ごとに arena 全部
    identical(&mut types, &artifacts.objects, &artifacts.packages, a, b)
}
```

同じ形が **`grep -rn "types\.clone()\|type_arena\.clone()"` で約 30 箇所**:
`unconvert` / `qf1008` / `gocritic::types_identical` / `printf` / `errorsas` / `stdmethods` /
`ifaceassert` / `nilnil` / `st1023` / `qf1012`(×2) / `sa4020` / `sa9004` / `qf1011` / `qf1005` /
`qf1010` / `gosec` / `exhaustruct` / `iface` / `errchkjson` / `contextcheck` / `sa9005` /
`nilerr` / `nilnesserr` ほか。

per-analyzer 表の上位はこれで説明がつきます（`QF1008` 1.23s / `unconvert` 0.84s /
`gocritic` 1.31s の一部）。**gocritic の `type_implements` は 2026-08 に memo 化済みですが、
すぐ下の `types_identical` は未対応のまま**で、そこにコメントが残っています
（「`TypeArena::clone` at 1.29s of self CPU」）。

### 1.3 `callcheck` が report ごとに AST 全走査する

`preorder::rec` 1.468s のうち **0.783s が `callcheck::run <- sa5009::run`**。犯人は:

```rust
// crates/guff-analysis/src/callcheck.rs:301
fn find_ast_call<'a>(files: &'a [File], kind: CallSiteKind, pos: Pos) -> Option<&'a CallExpr> {
    for file in files {
        preorder(NodeRef::File(file), |n| { /* pos が一致する CallExpr を探す */ });
    }
}
```

**1 件レポートするたびにパッケージの AST を頭から舐めています**（O(reports × nodes)）。

### 1.4 `callcheck` 本体を 25 本の analyzer が別々に回している

`callcheck::run(pass, rules())` を呼ぶ analyzer は **25 本**
（SA1000/1002/1003/1007/1010/1011/1014/1017/1018/1020/1021/1024/1026/1027/1028/1029/1030/1031/1032/
SA4015/SA5009/SA5012/SA6000/SA6002/SA9005）。
各々が **SSA プログラム全体を走査し、呼び出し先ごとに `type_func_name` で `String` を作って**
自分の rules ハッシュを引きます。`object_call_name` 0.354s + `type_func_name` 0.122s は
その 25 重掛けです。

### 1.5 コメント系リンタが同じファイルを何度も read + PARSE_COMMENTS する

`reparse_with_comments` の**実装が 4 本**あります:

| 実装 | キャッシュ | 使う人 |
|---|---|---|
| `guff-comment/src/util.rs` | **なし（毎回 `fs::read` + parse）** | godot, dupword, godox, godoclint |
| `guff-revive/src/util.rs` | thread-local + `pkg.source_bytes()` 再利用 | revive 6 ルール |
| `guff-style/src/gocritic.rs:2714` | なし | gocritic コメント系 |
| `guff-style/src/funlen.rs:24` | なし | funlen |

prometheus の設定では godot ✓ / gocritic ✓ / revive ✓ が同時に有効なので、
**同じファイルを最低 3 回、ディスクから読み直してフルパースしています**。
`godot` の per-analyzer 0.84s はほぼ全部これ（ロジック自体は文字列判定だけ）。
`__open` 0.969s + `read` 0.593s + `Scanner::scan/next` 0.99s の相当部分がここ。

### 1.6 `gocritic` はノードごとに `HashSet<String>` を SipHash で 114 回引く

```rust
fn enabled(set: &HashSet<String>, name: &str) -> bool { set.contains(name) }
```

`enabled(&set, "elseif")` 形式の呼び出しが **114 箇所**あり、**AST ノードを訪れるたびに
その種別に紐づく分だけ文字列ハッシュが走ります**。`guff-style` は `std::collections::HashMap`
（SipHash）のままで、V2 A-1 の FxHash 化対象から漏れています。
`hash_one` 0.231s + `sip::Hasher::write` 0.182s ≈ **0.41s** の大半がこれ。

---

## 2. 見積り（着手順の根拠）

| ID | 対象 | 除去できる CPU（20.07s 中） | リスク | 影響範囲 |
|---|---|---:|---|---|
| **V1-1** | `TypeArena` clone を O(1) 化 | **≈ 2.0–2.5s** | 中 | 約 30 チェック（コード無変更） |
| **V1-2** | kind バケット索引つき inspector | ≈ 0.3–0.8s | 低 | 149 呼び出し（コード無変更） |
| **V1-3** | `find_ast_call` を位置索引に | ≈ 0.8–1.0s | 低 | callcheck 25 本 |
| **V1-4** | コメント再パースの共有 | ≈ 0.8–1.0s | 低 | godot/gocritic/funlen/dupword/godox/godoclint |
| **V1-5** | callcheck の 25 重走査を 1 回に | ≈ 0.4–0.6s | 中 | staticcheck 25 本 |
| **V1-6** | gocritic の `enabled` をビットセット化 + guff-style FxHash | ≈ 0.3–0.4s | 低 | gocritic 106 checker |
| 合計 | | **≈ 4.6–6.3s CPU** | | |

analyze の実効並列度は約 7.7（CPU 14.5s / wall 1.89s）なので、CPU −5s は
**analyze wall で −0.5〜0.7s** 程度。cold wall 3.16s → **2.4〜2.6s** が目標です。
（V2 の baseline 2.36s に戻す、が第一関門。）

---

## 3. 検証プロトコル（V2 §2 の再確認 + 第3弾の追加）

各タスクで**必ず**:

1. **findings byte 一致**
   ```bash
   ./target/release/guff run -c prometheus/.golangci.yml --out-format json \
     --issues-exit-code 0 --no-cache ./... > after.json    # prometheus 内で
   diff <(jq -S . before.json) <(jq -S . after.json)       # 空であること
   ```
2. **決定性**: 同じコマンドを 5 回、`-j 1` でも 1 回、出力が同一。
3. **マスク健全性**（V1-2 のとき必須）: `GUFF_INSPECT_MASKS=0` と既定で findings 一致。
4. **本番ゲート**: `./regress/run.sh --profile tsdb` と `--profile full` の両方 PASS。
5. **compat golden**: `compat/` の golden / ratchet を壊していないこと。
6. **A/B/A/B 交互計測**（V2 §X-3）。この開発機は単発スパイクします。

### 3.1 `git checkout <file>` で他人（過去の自分）の変更を巻き添えにしない

**2026-08-14 に実際にやりました（§7.1.7）。** 1 つのファイルに 2 つのタスクの変更が
入っている状態で、片方を戻すつもりで `git checkout <file>` を打つと**両方消えます**。
しかもビルドは通り、findings も変わらないので**気づけません**。

- タスクを revert する前に `git diff <file>` を見て、**そのファイルに他のタスクの変更が
  同居していないか**確認する。
- 同居しているなら `git checkout` ではなく、当該ハンクだけを戻す。
- **revert 後は「入っているはずのもの」を 1 つずつ grep で確認する。**
  `grep -c 'let en_val_swap' crates/guff-style/src/gocritic.rs` の類で十分です。

### 3.2 このマシン固有の注意（2026-08-14 に再確認）

- `target/debug/deps` が **136,330 ファイル / 8.9 GB** まで肥大しており、
  `syspolicyd` が**常時 1.4 コアを焼いています**（`ps aux` で 143%）。
  ユーザーのメモ「cargo test が異常に遅いとき」と同じ状態です。
  **計測前に `scripts/perf-guard.sh` を必ず通すこと。** 通らないなら数字を信じない。

---

## 4. タスク詳細

---

## V1-1 — `TypeArena` のスクラッチ clone を O(1) にする（**最優先**）

### 目的

`artifacts.types.clone()` が **overlay（このパッケージ分の全 `TypeData`）と
`intern_overlay` を毎回ディープコピー**している。これを Arc 共有にして、
**呼び出し側のコードを 1 行も変えずに** 約 30 箇所を同時に直す。

### 現状の構造（`crates/guff-types/src/arena.rs`）

```rust
pub struct TypeArena {
    types: Layered<TypeData>,
    intern_base: Arc<HashMap<InternKey, TypeId>>,
    intern_overlay: HashMap<InternKey, TypeId>,
}

struct Layered<T> {
    base: Arc<Vec<T>>,   // export seed。全パッケージで共有
    overlay: Vec<T>,     // このパッケージが typecheck 中に作った型。**これが重い**
}
```

`clone()` は derive なので `base` は Arc bump、**`overlay` は Vec の実コピー**。

### やってはいけないこと

- **`freeze()` を使ってはいけない。** `freeze` は `Arc::make_mut(&mut self.base)` を呼ぶので、
  base（= 全パッケージ共有の export seed, C-8 実測で ~1.0 GiB）の refcount が 1 でなければ
  **seed 全体をディープコピーします**。パッケージごとに呼んだら破滅します。

### 設計（3 層化）

```rust
struct Layered<T> {
    base: Arc<Vec<T>>,     // 共有 seed（不変）
    mid:  Arc<Vec<T>>,     // このパッケージの typecheck 成果。share() で overlay から昇格
    overlay: Vec<T>,       // scratch clone 後の追記（インターン）だけがここに来る
}
```

- `get(i)`: `i < base.len()` → base / `i < base.len()+mid.len()` → mid / else overlay。
  分岐が 1 つ増えるだけ。
- `push`: 常に `overlay`。
- `share(&mut self)`: `overlay` を `mid` へ移す。**呼ぶのは typecheck 完了直後の 1 回だけ**で、
  そのとき `mid` の refcount は 1 なので `Arc::make_mut` はコピーしない。
- `clone()`: base + mid の Arc bump ＋ 空 overlay ＝ **O(1)**。
- `intern_*` も同じ 3 層化（`intern_base` / `intern_mid` / `intern_overlay`）。

### 手順

1. `Layered<T>` に `mid` を足し、`get` / `get_mut` / `len` / `push` / `overlay_len` /
   `into_overlay` / `extend_base` / `freeze` / `shared_clone` / `parts` を全部通す。
   **`freeze` / `shared_clone` / `extend_base` の既存呼び出し側（R25 のマージ経路）を壊さないこと。**
2. `TypeArena::share()`（+ `ObjectArena` などが必要なら同様）を追加。
3. `typecheck_package` 完了後（`type_artifacts` を組み立てる場所）で `share()` を 1 回呼ぶ。
4. `GUFF_DEBUG_CACHE=2` で `TypeArena::clone` の samply self が消えたことを確認。

### GO/NO-GO

samply の `TypeArena as Clone::clone` self が **1.0s 未満**にならなければロールバック。

### 検証

§3 のすべて。とくに **RSS が増えていないこと**（`mid` は overlay の移動なので増えないはず。
`/usr/bin/time -l` で peak RSS を baseline と比較）。

---

## V1-2 — inspector に kind バケット索引を持たせる

### 目的

`preorder_typed(mask, ..)` が**マスクに含まれる kind のイベントだけ**を辿るようにする。
202M scanned → 7M に落とす。

### 設計

`Events` に counting sort の結果を持たせる:

```rust
struct Events {
    nodes: Vec<Event>,
    /// kind ごとの開始位置（長さ NodeKind::COUNT+1）
    kind_off: [u32; NodeKind::COUNT + 1],
    /// kind でグループ化した `nodes` への添字。各グループ内は昇順＝preorder 順。
    by_kind: Vec<u32>,
    ...
}
```

- **1 kind**: そのバケットを直接舐める。順序は preorder 順のまま。
- **2〜3 kinds**: 添字の k-way マージ（k が小さいので単純な「最小 head を取る」ループで十分）。
- **それ以上 / `NodeMask::ALL`**: 従来どおり線形走査（マージのコストが勝つため）。

**閾値は実測で決めること。** 目安は「マスクに含まれる kind の合計イベント数が
全体の 1/4 未満ならバケット経路」。

### 不変条件（ここを外すと findings が壊れる）

- **返すノード列は従来と完全に同一**（同じノード、同じ順序）。
  バケット内は昇順で、マージは添字の小さい順なので preorder 順が保たれる。
- `events_for(files)` が `None`（per-file の `from_ref` 呼び出し）のときは
  **従来の再帰 walk のまま**。ここを変えない。

### 検証

§3 のすべて ＋ **`GUFF_INSPECT_MASKS=0` との findings 一致**（マスク経路を丸ごと殺した比較）。
`GUFF_DEBUG_CACHE=2` の `nodes scanned` が 202M から大きく落ちること。

### やってはいけない

- バケットを `HashMap<NodeKind, Vec<u32>>` にしないこと（56 種の固定配列で足りる）。
- 部分木スキップ（V2 B-1d）を再導入しないこと。**測って NO-GO 済み**です。

---

## V1-3 — `callcheck::find_ast_call` を位置索引に置き換える

### 目的

report ごとの AST 全走査（O(reports × nodes)）を O(1) にする。

### 設計

パッケージ単位で 1 回だけ、`CallExpr.lparen` / `DeferStmt.defer_` / `GoStmt.go_` の
オフセット → ノードの索引を作る。`callcheck` は `pass` 経由でそれを引く。

置き場所の候補（この順で検討）:
1. `callcheck` 内の thread-local キャッシュ（`pkg.id` キー）。revive の `REPARSE_CACHE` と同型で、
   既に前例がある。**最小の変更で済む。**
2. `inspect` の結果に相乗り（`InspectResult` に lazily built な `OnceCell` を持たせる）。

`InspectResult` のイベント配列があるので、索引作りは
`preorder_typed(node_mask!(CallExpr, DeferStmt, GoStmt), ...)` 1 回で済む
（V1-2 が入っていればさらに安い）。

### 検証

§3 のすべて。とくに **`find_ast_call` は「最初に見つかったもの」を返していた**ので、
同じオフセットに複数該当がある場合の**先勝ち**を索引でも再現すること
（`entry().or_insert()` で最初だけ入れる）。ここを取り違えると位置がずれます。

---

## V1-4 — コメント再パースを 1 パッケージ 1 回に共有する

### 目的

`fs::read` + `PARSE_COMMENTS` パースを、**有効なコメント系リンタの本数によらず
1 ファイル 1 回**にする。

### 現状

`reparse_with_comments` が 4 実装（§1.5 の表）。revive だけが thread-local キャッシュ +
`pkg.source_bytes()` 再利用を持っている。

### 設計

`guff-analysis` に**共有 analyzer**を 1 本足すのが筋（`inspect` と同じ形）:

```rust
// guff-analysis/src/passes/commentparse.rs
pub struct CommentParseResult { /* file index -> Arc<Reparsed> を lazy に */ }
```

- `requires: vec![]`、結果は `Arc` 共有なので action DAG が「パッケージごとに 1 回」を保証する。
- 各リンタ（godot / dupword / godox / godoclint / gocritic / funlen / revive）は
  自前の `reparse_with_comments` を捨ててこれを `requires` する。
- **`fs::read` ではなく `pkg.source_bytes(i)` を先に見ること**（typecheck が既に読んだバイト列）。
  revive の実装がそうなっているので合わせる。

**段階的にやること**: まず `guff-comment`（godot/dupword/godox/godoclint）だけ移す →
計測 → gocritic → funlen → revive の順。1 段ずつコミット。

### 注意（compat を壊す罠）

- 再パースは**専用の `FileSet`** を持ちます。位置を `pass` で報告する前に必ず
  既存の `line_pos` / `map_reparsed_pos` 相当で変換すること。
  共有化のときに「どの fset の Pos か」を取り違えるのが唯一かつ最大の事故です。
- `godot` は `declaration_docs`、`gocritic` は別の集め方をします。
  **共有するのは「パース結果」までで、収集ロジックは各リンタに残すこと。**

### 検証

§3 のすべて ＋ `__open` / `read` / `Scanner::scan` の samply self が落ちること。

---

## V1-5 — `callcheck` の 25 重走査を 1 回にする

### 目的

25 本の analyzer がそれぞれ SSA 全走査 + `String` 生成をしているのを、
**1 回の走査 + 25 回の安いフィルタ**にする。

### 設計

共有 analyzer `callindex`（`buildir` を `requires`）が、パッケージごとに 1 回:

```rust
pub struct CallIndex {
    /// 呼び出し先の完全名 -> その呼び出し地点
    pub by_name: HashMap<Arc<str>, Vec<CallSite>>,
}
struct CallSite { fid: FuncId, iid: InstrId, kind: CallSiteKind, target: ObjectId }
```

`callcheck::run(pass, rules)` は `rules` のキーだけを `by_name` から引き、
**該当がある名前についてのみ** `build_call` して check を回す。
`type_func_name` の `String` 生成は 25 回 → 1 回になる。

### GO/NO-GO

先に「25 本のうち prometheus で 1 件でもヒットする rules は何本か」を数えること。
ほとんどの analyzer は**該当呼び出しゼロ**のはずで、その場合この変更は
「全走査 25 回 → 全走査 1 回 + ハッシュ 25 回」になり効果が大きい。

### 注意

- **報告順序**。今は analyzer ごとに `src_funcs_with_methods()` 順で `pending` に積み、
  最後に emit している。`by_name` から引くと順序が変わるので、
  **`CallSite` を `(fid, iid)` 昇順にソートしてから回すこと**。
  ここを外すと findings の並びが変わり、golangci との diff が出ます。

---

## V1-6 — gocritic の `enabled()` をビットセットにする + `guff-style` を FxHash 化

### 目的

AST ノードごとの `HashSet<String>` SipHash 引き（114 箇所）を、
**run 開始時に一度だけ引いた `u128` ビットセット**（または `[bool; N]`）に置き換える。

### 設計

```rust
// checker 名 -> 連番 index を const で持つ
const CHECKS: &[&str] = &[/* implemented_checks() と同じ順 */];
struct EnabledSet([u64; (CHECKS.len() + 63) / 64]);
impl EnabledSet { #[inline] fn has(&self, id: CheckId) -> bool { ... } }
```

`enabled(&set, "elseif")` → `set.has(CheckId::ElseIf)`。
`enabled_set()` は 1 回だけ文字列で解決し、ビットを立てる。

**機械的な置換で 114 箇所**なので、`CheckId` は `enum` にして
「名前 → id」を 1 箇所（`from_name`）に閉じ込めること。名前のタイポは
**`implemented_checks()` との突き合わせテストで落とす**（今は文字列なので静かに false になる）。

あわせて `guff-style` の `std::collections::{HashMap, HashSet}` を V2 A-1 と同じ手順で
FxHash 化する。**先に V2 §0-12（iteration order 依存の洗い出し）をやること。**

### 検証

§3 のすべて。findings 一致は**必須**（enable/disable の解釈がずれたら即バレる）。
`enable-all: true` と既定の両方で確認すること。

---

## 4.5 次にやる人へ — V1 完了後の地図（2026-08-14）

**直列 phase は 2.85s。内訳は analyze 1.13s / typecheck_roots 1.14s / load_graph 0.53s
で、初めて 3 つがほぼ同じ大きさになりました。**「ここを削れば効く」という単独の山は
もうありません。

samply（`perf-v3`、総 CPU 17.8s、prometheus `./...` cold）の self 上位は:

| CPU | % | symbol | 読み方 |
|---:|---:|---|---|
| 1.22s | 6.8% | `_platform_memmove` | 最大の呼び元は `code::func_name` の `format!` |
| 0.98s | 5.5% | `__open` | `load_package_files`（native lister）+ `build_source_seed_inner` |
| 0.64s | 3.6% | `Scanner::scan` | 本物のパース |
| 0.55s | 3.1% | `read` | 同上 |
| 0.54s | 3.0% | `Scanner::next` | 同上 |
| 0.52s | 2.9% | `walk::inspect::rec` | **gocritic の自前再帰 walk**（inspector を使っていない） |
| ~1.2s | ~7% | `mi_free` / `mi_malloc_aligned` / `mi_page_free_list_extend` | アロケータ |

### 着手候補（期待値の高い順）

1. ~~**`code::func_name` の `String` を作らない**~~ → **NO-GO 済み（§7.1.6）。着手しないこと。**
   実装して測ったら第1版は CPU +6.5%、第2版で誤差帯でした。
2. ~~**gocritic を inspector に載せる**~~ → **DONE（V1-12）。**
   「部分木を刈っているから単純移行はできない」と書きましたが、**確かめたら刈っていませんでした** —
   クロージャに `return false` は 1 つも無く、末尾も `true` です。加えて `walk::inspect` は
   復路にも `f(None)` を撃つのに、このクロージャはそれを `return true` で捨てるだけなので、
   **呼び出しの半分が純粋な無駄**でした。gocritic CPU −3.3%。
   *教訓: 「〜だからできない」は仮説です。grep で 30 秒で確かめられました。*
3. **`typecheck_roots` 1.14s** — V2 §B-9（wave バリア撤廃）は原則着手しない、が結論のまま。
   ただし今や analyze と同じ大きさなので、**再評価する価値は出てきました**。
4. **`load_graph` 0.53s** — `__open` はプロファイル 1 位（8.5%）ですが、
   **優先度は低いと判断しました**。`load_package_files` のヘッダ読みは
   GOMODCACHE / GOROOT 分が `modmeta` に恒久キャッシュされるので（V2 §C-3c Phase 3）、
   **この 0.53s は「`GUFF_CACHE` が空の初回だけ」の数字**です。
   本ファイルの計測はすべて毎回 `mktemp -d` で cold にしているのでフルに出ていますが、
   実利用の 2 回目以降は V2 実測で load **0.04s** です。
   *プロファイルの 1 位が最優先とは限らない、の実例。*
5. **アロケータ ~6%**（`mi_free` / `mi_malloc_aligned` / `mi_page_free_list_extend`）と
   `drop_in_place<Expr>` 0.31s / `Expr::clone` 0.15s。AST のクローンと破棄です。
6. **同種の O(候補 × AST) 探し。** V1-13 は「1 候補ごとにパッケージ全走査」でした。
   同型が他にもあります（`grep -n "for file in pass.files()" -A2 | grep walk`）。
   **ただし着手前に必ずプロファイルで裏を取ること** — V1-11 は「コードを読んで怪しい」
   だけで着手して 1 時間を失いました。

### やってはいけない

- **`format_checks` に手を出さない。** `waited=0.00s` で余裕 2.2s、まだ内側に隠れています（V2 §B-10）。
- **プロファイルを取り直さずに次を選ばない。** 第3弾の最大の一撃（V1-7, 2.9s）は、
  V1-1〜V1-3 を入れて**測り直したときに初めて 1 位に現れました**。

---

## 5. Tier V2 — 個別 analyzer（V1 を入れてから測り直して着手）

**V1 を全部入れた後の per-analyzer 表を取り直してから着手すること。** V1-1 だけで
`QF1008` / `unconvert` / `gocritic` の相当部分が消えるはずなので、下の見立ては前提が変わります。

| 候補 | 2026-08-14 の CPU | 見立て |
|---|---:|---|
| `buildir` | 1.01s | V2 B-2 で lazy import members 済み。残りは実測から |
| `SA1019` | 0.78s | `dep_facts` 0.176s self。facts の引き回しを見る |
| `revive` | 0.72s | V2 B-4 で shared_walk 化済み。V1-4 の効果を先に測る |
| `modernize` | 0.29s | |
| `misspell` | 0.28s | |
| `fact_deprecated` | 0.24s | |

---

## 6. Tier V3 — パイプライン / ビルド

| ID | 内容 | 状態 |
|---|---|---|
| V3-1 | PGO を配布ビルドの既定にする | `scripts/build-pgo.sh` は V2 A-8b で常設済み。**CI / release に組み込まれていない**。`-j1` wall −0.28s の実績あり |
| V3-2 | `typecheck_roots` 1.23s の再分解 | V2 B-9（wave 撤廃）は原則着手しない。C-7 speculate は導入済み |
| V3-3 | `format_checks` 0.74s の並列度 | 現状 `waited=0.00s` で内側に隠れている。analyze が縮むと表に出るので **V1 完了後に再測** |
| V3-4 | `codegen-units=1` / `lto="fat"` 済み。`panic="unwind"` は `catch_unwind` のため据え置き | 触らない |

---

## 7. 進捗

作業ブランチ: `perf-v3`（`953d243` から分岐）。**すべて findings バイト同一**
（`Pos.Offset` のみ除外。これは FileSet の割当順に依存する値で、golangci 比較でも
`compat/normalize.py` が見ていない）。

| ID | 状態 | 実測 |
|---|---|---|
| V1-1 TypeArena scratch clone | **DONE** | `TypeArena::clone` 1.36s → プロファイル上位から消滅 |
| V1-2 kind バケット索引 | **DONE** | nodes scanned **466.8M → 20.7M（−95.6%）**、delivered は 15.16M で不変 |
| V1-3a callcheck の空 report を捨てる | **DONE** | `walk::preorder::rec` 1.47s → 0.28s |
| V1-4 コメント再パース共有 | **NO-GO** | RSS **+0.94 GiB**、wall **+1.1%**。§V1-4 参照 |
| V1-4' 再パースに既読バイトを渡す | **DONE** | `read` 0.59s → 0.12s |
| V1-5 callcheck の呼び出し名メモ化 | **DONE** | `object_call_name` を 25 重 → 1 重に |
| V1-6 gocritic の `enabled` 巻き上げ | **NO-GO** | CPU **+0.5%**。§7.1.7（**一度 DONE と誤記していました**） |
| **V1-7 `FileSet` の last キャッシュを thread-local 化** | **DONE** | **`__psynch_mutexwait`+`drop` 2.9s（14.3%）が消滅。第3弾で最大の一撃** |
| V1-8 walk 内の `Regex::new` を排除 | **main が同着** | `Regex::new` 2.11s inclusive（6.6%）→ 消滅。§7.1.8 |
| **V1-2b 部分スライスも索引で配る** | **DONE** | `from_ref(file)` 経路（S1008 ほか 5 箇所）が再帰 walk に落ちていた。20.7M → **18.0M scanned** |
| V1-9 `filter_debug` の `Vec` 割当を除去 | **DONE** | 走査のみの 3 箇所を `iter_non_debug` へ |
| V1-10 S1008 の再パース/位置引き | **DONE** | `fs::read` → `source_bytes`、`rfset.file()` をコメント毎 → ファイル毎に |
| V1-11 `is_call_to` の `String` 割当除去 | **NO-GO** | 第1版 CPU **+6.5%** / 第2版 −0.1%（§7.1.6） |
| V1-12 gocritic を共有 inspector へ | **DONE** | gocritic CPU **−3.3%**（§4.5 候補 2 の答え合わせ） |
| V1-13 modernize の「代入されるか」を索引化 | **DONE** | modernize CPU **−12.5%**、O(loops × AST) → O(AST) |

### 7.1 V1-7 は計画に無かった —「詰めたら出てきた」項目

V1-1〜V1-3 を入れて**プロファイルを取り直したら**、それまで 15 位圏外だった
`__psynch_mutexwait` が **1 位（10.2%）** に浮上しました。犯人は

```rust
// crates/guff-ast/src/position.rs（V1-7 前）
fn file_internal(&self, p: Pos) -> Option<Arc<File>> {
    let last = self.last.lock().unwrap();   // ← FileSet 全体で 1 個の Mutex
    ...
}
```

`FileSet::position()` は**全 analyzer が全診断・全位置解決で呼ぶ**関数で、その
「直前に引いたファイル」キャッシュが **FileSet 単位の `Mutex` 1 個**でした。
rayon ワーカー 10 本がここに並び、しかもスロットが 1 個しかないので
**互いのエントリを追い出し合ってキャッシュはほぼ常にミス**していました。

thread-local 2 スロット（共有 fset と再パース用 fset の往復に耐える）に変更し、
`remove_file` / `insert_files` 用に generation カウンタで無効化します。
あわせて `File.mutable` を `Mutex` → `RwLock`（書きは parse 中の `add_line` だけ、
以降は読み専用）。

**教訓（V2 §0-11 の系）: 上位を 1 つ潰すたびにプロファイルを取り直すこと。**
第2弾で `A-4`（`File::add_line` の Mutex 除去）が NO-GO になったのは、当時
この Mutex が他の仕事に隠れていたからです。**同じ場所が、隠していたものを
取り除いた途端に 1 位になりました。**

### 7.1.5 実測（`953d243` vs `perf-v3`、同一マシン・同一セッション）

**wall（A/B/A/B 交互、クリーンなマシン、prometheus `./...`, cold, `--no-cache`）**

**3 回、別々のタイミングで取り直して同じところに落ちます**:

| 計測 | HEAD | perf-v3 | 差 |
|---|---:|---:|---:|
| V1-1〜V1-10 時点（6 往復） | 4.435s | 2.960s | **−33.3%** |
| 同・コミット後の再確認（6 往復） | 5.050s | 3.250s | **−35.6%** |
| V1-12/V1-13 後（6 往復） | 4.980s | **3.215s** | **−35.4%** |

RSS は 3.24 GiB → 3.23 GiB で**変化なし**（むしろわずかに低い）。

> HEAD 側の絶対値が 4.4〜5.1s とぶれているのは、この開発機に他エージェントの
> Go ビルドが断続的に乗るためです。**交互実行なので比だけは安定します**（§3.2）。

**phase 内訳**（同じ条件で `GUFF_DEBUG_CACHE=2`、A→B を続けて実行）

| phase | HEAD | perf-v3 | 差 |
|---|---:|---:|---:|
| startup | 0.00s | 0.00s | — |
| load_graph | 0.57s | 0.53s | −0.04 |
| cache setup+partition | 0.00s | 0.00s | — |
| typecheck_roots | 1.21s | 1.12s | −0.09 |
| **analyze** | **2.72s** | **1.11s** | **−1.61（−59%）** |
| issues+filter | 0.05s | 0.05s | — |
| **直列合計** | **4.55s** | **2.81s** | **−1.74（−38%）** |
| format_checks（内側で並走） | 0.72s | 0.66s | −0.06 |

**inspector の走査量**: 466,830,871 → **19,544,005 nodes scanned**（−95.8%）。
delivered（15.16M → 15.78M）が増えているのは V1-12 で gocritic が inspector 経由になり
**その分が計上されるようになった**ためで、gocritic 自身の走査量が増えたわけではありません。

> **format_checks はまだ `waited=0.00s`** です。直列 2.85s に対して format 0.67s なので、
> 余裕は約 2.2s。**B-10 の追加作業（V2 §B-10）は今も着手条件を満たしていません。**

**regress ゲート**（`./regress/run.sh`, 同一セッションで両方を実行）

| profile | HEAD wall | perf-v3 wall | 差 | findings | peak RSS |
|---|---:|---:|---:|---|---|
| `tsdb` | 1.920s | **1.190s** | **−38%** | 7/4/3 で同一 | 1.225 GB → 1.235 GB |
| `full` | 6.610s | **3.020s** | **−54%** | 24/20/4 で同一 | 3.420 GB → 3.381 GB |

> **両 profile とも FAIL のままですが、FAIL の中身は HEAD と同一です。**
> `guff_only`（tsdb 3 / full 4）と RSS 超過は **`953d243` 時点で既に存在する**もので、
> baseline.json はワーキングツリーの未コミット compat 修正込みで記録されているためです。
> **`wall_seconds` の FAIL 幅は HEAD の半分以下に縮みました。**
> 未コミット分をマージすれば findings 側の FAIL は消えるはずです（本ブランチの変更は
> findings に対して no-op なので干渉しません）。

**OSS コーパス（A/B/A/B 交互 3 往復、`./...`、cold、`--no-cache`）**

| target | HEAD | perf-v3 | 差 |
|---|---:|---:|---:|
| gin | 0.551s | 0.363s | **−34.1%** |
| caddy | 0.861s | 0.774s | −10.1% |
| cobra | 0.235s | 0.222s | −5.5% |
| k9s | 1.920s | 1.845s | −3.9% |
| helm | 1.380s | 1.370s | −0.7% |

> **効き幅は「その設定がどれだけ analyze を回すか」に比例します。**
> prometheus の設定は `gocritic: enable-all` / `govet: enable-all` / staticcheck 全部 /
> revive 25 ルール / godot / testifylint `enable-all` …と analyze が重く、−33%。
> helm / k9s は有効リンタが少なく load_graph と typecheck が支配的なので、
> 今回の改善はほとんど乗りません。**これは想定どおりで、次の一手が
> typecheck / load_graph 側にあることを示しています（§9）。**
>
> `benchmarks/run.sh --oss --tier pr` の perf ゲートは PASS。golangci-lint 比は
> cold 6.0〜11.4x / warm 11.0〜27.2x（`benchmarks/results/SCOREBOARD.md`）。
>
> ⚠️ `benchmarks/run.sh` は**片方のバイナリの全サンプルを流し切ってからもう片方**を回すので、
> 途中で別プロセスが立ち上がると偽の回帰が出ます（2026-08-14 に `mediaanalysisd` で実際に発生し、
> helm が「+47%」に見えました）。**A/B 比較には交互実行を使うこと。**

**検証（全部 PASS）**

| 検証 | 結果 |
|---|---|
| findings バイト同一（並列, `./...`） | ✅ HEAD ≡ perf-v3（24 件・順序込み） |
| findings バイト同一（`-j 1` + `RAYON_NUM_THREADS=1`） | ✅ HEAD ≡ perf-v3 |
| `GUFF_INSPECT_MASKS=0` ≡ 既定 | ✅ 同一（V1-2 のマスク経路の健全性） |
| 決定性（同一バイナリ 5 回） | ✅ 同一 |
| `cargo test --release --workspace` | ✅ **3,116 passed / 0 failed** |
| `compat/golden/run.sh`（check レベル golden） | ✅ **OK: 81 case(s) match golden exactly**（staticcheck-sa/st の ratchet も baseline どおり） |

> V1-2b で `passes::inspect::tests::foreign_file_slice_falls_back_to_walking` が
> 1 本落ちました。**「単一ファイルのスライスは fallback する」という旧実装の性質を
> assert していた**テストなので、新しい契約に合わせて
> `single_file_subslice_uses_the_flat_index`（部分スライスの列が直接 walk と完全一致することを
> 確認）へ書き換え、fallback 側は「そもそも別の allocation のファイル」で検証するよう
> 直しました。**意味的な assert（`first == direct` など）は落ちていません。**

### 7.1.6 V1-11 NO-GO — `is_call_to` の `String` 割当は「犯人」ではなかった

§4.5 の候補 1 番（`code::func_name` の `format!` を消す）を**実装して測って NO-GO** にしました。
記録として残します。

`is_call_to` / `is_call_to_any` は 75 箇所から呼ばれ、いずれも
`"import/path.Func"` を `format!` で組んでリテラルと比較し、すぐ捨てています。
プロファイル上 `code::func_name` は `_platform_memmove`（1.22s / 6.8%）の最大の呼び元でした。

**第1版**: `object_call_name_is(pass, obj, want) -> bool` を足して割当を廃止。
→ **CPU +6.5%（中央値 20.88s → 22.23s、5 往復すべてで悪化）。**
理由は明白で、`is_call_to_any(pass, call, LOCK_CALLS)` が**名前 1 つごとに**
`objects.get` / `obj.pkg` / `packages.get(pkg).path()` をやり直していました。
**1 回の割当を N 回のアリーナ参照に置き換えていた**わけです。

**第2版**: オブジェクトの解決を 1 回に巻き上げ、`(path, name)` を借りたまま N 回比較。
→ **CPU −0.1%（18.05s → 18.04s）＝ 誤差帯。**

つまり **`String` の割当自体は mimalloc がほぼ吸収しており、削っても wall にも CPU にも出ません。**
プロファイルで `func_name` 経由の `memmove` が見えていたのは、大半が
**callcheck の `type_func_name`**（メソッド受信側の型を `type_string` で描画する重い方）で、
そちらは **V1-5 のメモ化で既に解決済み**でした。

**振る舞いに影響しない・計測で出ないリファクタは入れない**（V2 §0-14）ため、revert しました。
**§4.5 の候補 1 番は消し込み済みです。次の人は 2〜5 番から選んでください。**

### 7.1.7 V1-6 NO-GO — そして「入れたつもりで入っていなかった」記録

**この節は失敗の記録です。同じ事故を避けるために残します。**

V1-6（gocritic の `enabled(&set, "x")` 99 個を run 先頭に巻き上げ）は一度実装し、
findings 同一を確認し、**DONE として表に書き、コミットメッセージにも書きました**。
しかしその後 V1-4 を revert するとき

```bash
git checkout crates/guff-style/src/gocritic.rs   # V1-4 の変更を戻すつもり
```

を実行しており、**同じファイルに入っていた V1-6 も一緒に消えていました**。
以降の計測はすべて V1-6 **なし**の状態で取られています。

気づいたのは §4.5 の候補 2（gocritic を inspector に載せる）を調べようとして
`gocritic.rs` を開いたときで、`enabled(&set, ...)` が 114 箇所そのまま残っていました。

**そこで改めて入れ直して測ったところ、NO-GO でした**（`abcpu.sh`、5 往復交互）:

```
V1-6 なし CPU: median 18.48s
V1-6 あり CPU: median 18.58s      → +0.10s (+0.5%)、min-to-min +0.51s
```

理由の見立て:
- prometheus の設定は `gocritic: enable-all` なので**ほとんどの checker が有効**＝
  どのみち分岐は成立し、飛ばせる仕事がない。
- 短いリテラルの `HashSet<String>::contains` は分岐予測とキャッシュにほぼ吸収される。
- 巨大な関数に `let` を 99 個足すのはレジスタ圧の面でむしろ不利。

**そもそも根拠の読み違いでした。** プロファイルの SipHash 0.264s は、
callers を見ると最大の呼び元が **`guff_unused::run`** であって gocritic ではありません。
「gocritic がノードごとに 114 回ハッシュしている」は**コードから推測しただけで、
プロファイルで裏を取っていませんでした**（V2 §0-14 違反）。

**したがって §7.1.5 の実測値（−33.3% / −35.6%）に V1-6 は含まれていません。**
数字自体は V1-6 なしで取られているので**正しいまま**です。
`b012b69` のコミットメッセージだけが V1-6 を含むかのように書かれています。

### 7.1.9 rebase 後（`b5dbcb8` 比）の実測 — **これが現在の数字**

`953d243` → `b5dbcb8` の間に main 側で compat 修正と V1-8 相当が入ったので、取り直しました。

**wall（A/B/A/B 交互 6 往復）**

```
main   : median 3.445s   (3.20, 3.38, 3.41, 3.51, 3.48, 3.50)   RSS 3.20–3.23 GiB
perf-v3: median 2.745s   (2.64, 2.71, 2.71, 2.78, 2.78, 2.83)   RSS 3.16–3.22 GiB
                                                  → -0.700s / -20.3%
```

**phase 内訳**

| phase | main | perf-v3 | 差 |
|---|---:|---:|---:|
| load_graph | 0.54s | 0.55s | +0.01 |
| typecheck_roots | 1.15s | 1.19s | +0.04 |
| **analyze** | **1.59s** | **0.95s** | **−0.64（−40%）** |
| format_checks（並走） | 0.89s | 0.71s | −0.18 |

**inspector 走査**: 198,249,510 → **11,138,708**（**−94.4%**）。
delivered が 6.75M → 7.38M と増えているのは V1-12 で gocritic が inspector 経由になり
計上されるようになったためで、gocritic の走査量が増えたわけではありません。

**regress `--profile full`**: `guff_only` **0**、precision/recall **1.0000**（main と同じ）。
`wall_seconds` だけ baseline 超過で FAIL ですが、**main 3.210s に対し本ブランチ 2.930s** で、
超過幅は 0.85s → 0.42s と半減します。baseline `2.360s` は 2026-07-30 時点の値なので、
**マージ後に `--update-baseline` を回すのが筋**です。

### 7.1.8 V1-8 は main と同着だった（rebase 時に判明）

本ブランチを `b5dbcb8` に rebase したとき、`testifylint.rs` だけが衝突しました。
中身を見たら **main 側も同じ修正を独立に入れていました** — walk の中で毎回
`Regex::new` していたのを thread-local の `cached_pattern` にする、まったく同じ形です。
理由の書き方まで似ています（あちらは「regex *construction* が ~0.53s、
run 中の全 regex マッチを合計したより多い」）。

**main 側を採用**して本ブランチの重複を落としました。同じ結論に別々に到達したので、
この最適化については確度が高いと言えます。

### 7.2 V1-4 NO-GO の詳細

`commentparse` analyzer を作り、godot / gocritic / funlen / dupword / godox /
godoclint が 1 パッケージ 1 回の `PARSE_COMMENTS` を共有する形にしました。
実装は動き findings も同一でしたが:

| | R3（V1-4 前） | R4（V1-4 後） |
|---|---:|---:|
| wall 中央値 | 4.610s | 4.660s（**+1.1%**） |
| peak RSS | 3,206–3,234 MiB | 4,162–4,184 MiB（**+0.94 GiB**） |

理由は単純で、**旧コードは 1 ファイル分の AST しか同時に持っていなかった**
（`for i in 0..n { let (fset, parsed) = reparse(...); ... }` で毎周捨てる）のに対し、
共有結果は**パッケージ全ファイル分を analyze フェーズの間ずっと保持**するためです。
`regress` の RSS 許容は `peak_rss_ratio: 1.2` なので、これは**ゲート違反**です。

**残したもの**: `reparse_with_comments(path, cached)` に既読バイトを渡す変更
（`guff-comment` 4 本 / `gocritic` / `funlen`）。RSS ゼロコストで `read` が消えます。

---

## 8. 「二度と太らせない」ための仕掛け（V0）

第3弾で一番効くのは、実は個々の最適化ではなく**再発防止**です。
analyze が 0.37s → 1.89s になった 2 週間、**誰もそれに気づけませんでした**
（tsdb プロファイルは PASS し続けたため）。

### V0-1 — per-analyzer CPU 予算のラチェット

`GUFF_DEBUG_CACHE=1` の per-analyzer 表を機械可読で吐き、
`regress/` にチェックインした予算ファイルと突き合わせて**予算超過で FAIL** させる。

- 予算は「CPU 秒」ではなく **analyze CPU 全体に対するシェア** にすること
  （マシンが変わっても比較できる）。
- 新規 analyzer は予算行の追加を必須にする。**追加時にコストを目視させるのが狙い。**

### V0-2 — full プロファイルを CI の必須ゲートにする

`./regress/run.sh --profile full` は今 FAIL しています。tsdb だけを見ていると
`./...` 固有の回帰（＝依存グラフが太いときだけ出るもの）が見えません。

---

## 付録 A — 再現コマンド

```bash
# ビルド
cargo build --release -p guff-lint
cargo build --profile profiling -p guff-lint    # samply 用（strip なし）

# 計測環境の確認（通らないなら数字を信じない）
scripts/perf-guard.sh

# phase + per-analyzer 内訳
cd prometheus
C=$(mktemp -d); GUFF_CACHE=$C GOLANGCI_LINT_CACHE=$C GUFF_DEBUG_CACHE=2 \
  /usr/bin/time -l ../target/release/guff run -c .golangci.yml --out-format json \
  --issues-exit-code 0 --no-cache --timeout 15m ./... >/dev/null

# samply プロファイル（ヘッドレス集計）
C=$(mktemp -d); GUFF_CACHE=$C samply record --save-only --unstable-presymbolicate \
  -o /tmp/guff.json.gz -- ../target/profiling/guff run -c .golangci.yml \
  --out-format json --issues-exit-code 0 --no-cache ./... >/dev/null
python3 ../scripts/perf-profile.py /tmp/guff.json.gz --top 45

# ゲート
./regress/run.sh --profile tsdb
./regress/run.sh --profile full
```
