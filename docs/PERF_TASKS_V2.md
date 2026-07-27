# guff 高速化タスク 第2弾 — 実装エージェント向け詳細手順書

> **このファイルを読む前に、必ず `docs/PERF_TASKS.md` を最初から最後まで読むこと。**
> あちらは「第1弾（Task 1〜5）」の手順書兼作業記録で、**絶対ルール（§0）・計測ハーネス（§1）・
> findings 同一性の検証（§2）・本番ゲート（§3）はすべてこちらでもそのまま有効**です。
> このファイルは重複を避けるため、それらを再掲せず参照します。
>
> こちらは **2026-07-27 にコードベース全体を再調査して洗い出した「次の 33 本」** です。
> 第1弾のタスクはすべて完了（DONE）しているので、新しく着手するならこのファイルから選びます。

---

## 0. 追加の絶対ルール（`PERF_TASKS.md` §0 の10個に加えて）

第1弾の10ルールは全部そのまま生きています。以下は今回の調査で「これも明文化しないと事故る」と
判明したものです。

### ルール 11 — 「全 phase が均等に遅い」なら、それはあなたのコードのせいではない

今回の調査で実際に踏みました。詳細は §1.1。**1つの phase だけが遅いのか、全部が同じ倍率で
遅いのかを必ず見ること。** 全部が同じ倍率なら原因はマシン（他プロセスの CPU 食い・熱・電源）
であって、コードではありません。ここを間違えると、ありもしない回帰を追いかけて何時間も溶かします。

判定方法は §1.2 のスクリプトを**計測のたびに**走らせること。

### ルール 12 — `HashMap` のハッシャを差し替える前に、iteration order 依存を全部潰す

`std::collections::HashMap` は実行ごとにシード（`RandomState`）が変わるので、
**イテレーション順に依存したコードは今すでに非決定的**です。今それが顕在化していないのは
「たまたま順序が結果に影響しない」か「後段でソートしている」から。

ハッシャを `FxHashMap` に替えると順序が**決定的だが今までと違う順序**になります。
これで隠れていたバグが顕在化して findings が変わることがあります。これは
「あなたが壊した」のではなく「元々壊れていたのが見えた」ケースですが、**どちらにせよ
findings が変わったらコミットしてはいけません**（§0-10）。原因を特定して、
順序依存そのものを直してから再挑戦します。

差し替え前に必ずやること:

```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff
# HashMap を直接 iterate している箇所を全部列挙して、順序依存がないか目で見る
rg -n 'for .* in .*\.iter\(\)|for .* in &\w+_map|\.values\(\)|\.keys\(\)|\.drain\(\)' \
  --glob '*.rs' crates | rg -i 'map|table|cache|registry|index' | head -50
```

順序依存を見つけたら **そこを直すのが先**（`BTreeMap` にするか、collect 後に sort する）。

### ルール 13 — 参照実装（Go 本体 / golangci-lint / staticcheck）に既にある最適化を優先する

guff は Go のツール群の移植です。「Go 側では最適化されているのに guff では素朴なまま」の箇所が
一番安全でリターンが大きい（**設計が既に検証済みで、findings が変わらないことも実証済み**）。
逆に guff 独自の新発明は、findings を壊すリスクが跳ね上がります。

このファイルの Tier B の目玉（B-1: 本物の Inspector）はまさにこのパターンです。

### ルール 14 — 「効くはず」で実装を始めない。必ず先に**上限を測る**

各タスクに「**GO/NO-GO 判定**」節を用意してあります。そこに書いてある計測を先にやって、
削減できる上限（＝その処理に費やしている総時間）を確認してから実装に入ってください。
上限が wall 換算 0.1s 未満なら、そのタスクは **やらない**のが正解です。
第1弾 §1.9 の末尾（「parse の残りを詰めるべきか（結論: 詰めない）」）が良いお手本です。

### ルール 15 — 1タスク = 1コミット = 1 regress PASS

`PERF_TASKS.md` §0-9 の再掲ですが、第2弾はタスク数が多いので改めて。
**§4 のタスクテンプレートをコピーして、チェックが全部埋まってからコミット**してください。

---

## 1. 計測（第1弾 §1 の更新・補足）

計測ハーネス本体（cold / warm のコマンド）は `PERF_TASKS.md` §1.2〜§1.5 をそのまま使います。
ここでは**今回追加で分かったこと**だけ書きます。

### 1.1 実例: 計測汚染がどう見えるか（2026-07-27 の実測・**対照実験つき**）

**同じバイナリで、Chrome を開いたまま 3 回・Chrome を閉じて 3 回**まわした結果です。

| phase | 2026-07-26 のクリーン値（`PERF_TASKS.md` §1.10） | 汚染時（Chrome 起動中） | **クリーン時（Chrome 終了後）** |
|---|---:|---:|---:|
| load_graph（`go list`） | 1.53s | 2.55〜2.69s | **1.25s** |
| typecheck_roots | 1.79s | 4.56〜4.72s | **1.86〜1.90s** |
| └ seed dep check | 1.40s | 3.49〜3.65s | **1.41〜1.43s** |
| analyze | 1.16s | 2.56〜2.66s | **1.20〜1.23s** |
| format_checks | 0.59s | 4.99〜5.14s | **1.70〜1.79s** |
| issues+filter | 0.04s | 0.07s | **0.03〜0.04s** |
| **wall** | **4.88s** | **10.55〜11.00s** | **4.71〜4.79s** |
| **peak RSS** | **7.73GB** | **7.69〜7.76GB** | **7.60〜7.72GB** |
| findings | 20 | 20 | **20** |

汚染時の負荷はこれでした:

```
load averages: 5.13 3.90 3.31       ← 10 コア（性能4 + 効率6）機で load 5
84.5% Google Chrome
59.3% Google Chrome Helper
33.2% Google Chrome Helper
22.7% Cursor Helper
```

Chrome を閉じたあとは `load averages: 2.05 2.34 2.61` になり、**wall が 10.6s → 4.75s に戻りました。**
コードは 1 行も変えていません。

**この事例から必ず持ち帰ってほしい 3 点:**

1. **RSS を見れば汚染かどうか一発で分かる。** 汚染時も wall は 2.2 倍なのに **RSS は 3 桁一致**
   しています。RSS はやる仕事の量で決まり、CPU が空いているかどうかでは変わりません。
   「**やっている仕事は完全に同じで、CPU だけが足りていない**」の決定的な証拠です。
   **wall が悪化して RSS が変わっていなければ、まずマシンを疑う。**
2. **ブラウザとエディタも `cargo build` と同罪。** `PERF_TASKS.md` §0-7 は
   「`cargo build` が走っていないか確認しろ」としか書いていませんが、不十分でした。
3. **「全 phase が揃って同じ倍率」がシグナル。** 1 つの phase だけ突出して悪いなら、
   それは本物の回帰である可能性が高い。→ **まさにそれが `format_checks` でした（P0-1）。**
   クリーン環境でも他の全 phase が §1.10 の記録値と一致するなか、
   **format_checks だけが 0.59s → 1.75s と 3 倍**残りました。汚染では説明できません。

### 1.2 計測前に必ず走らせるガード（S-1 で常設化する）

S-1 タスク（§5）を実装するまでは、以下を手で打ってください。

```bash
# 1) 重いプロセスが居ないか
ps aux | sort -nrk3 | head -8 | awk '{printf "%5s%%  %s\n", $3, $11}'
# 2) load average が (コア数 / 4) 以下か
uptime
sysctl -n hw.ncpu
# 3) 低電力モード / 熱スロットルに入っていないか
pmset -g | grep -i lowpowermode
pmset -g therm
# 4) 自分以外のビルド/エージェント
ps aux | grep -iE 'cargo|rustc|cursor-agent|go build' | grep -v grep
```

**合格ライン: load average < ncpu/4（10 コアなら 2.5 未満）、CPU 上位に自分のプロセス以外が居ない、
lowpowermode = 0。** 満たさないなら Chrome を閉じるか、落ち着くまで待つ。

**実測での較正値（この開発機、Darwin arm64 / 10 コア = 性能4 + 効率6）:**

| 状態 | load avg (1min) | cold wall | 判定 |
|---|---:|---:|---|
| Chrome + Cursor + Activity Monitor 起動 | 5.13 | 10.6s | **FAIL**（数字は使い物にならない） |
| Cursor のみ（Chrome 終了後） | **2.05** | **4.75s** | **PASS**（`PERF_TASKS.md` §1.10 の記録値を再現） |

つまり **Chrome を閉じるだけで十分**でした。Cursor は起動したままで問題ありません。
とはいえ 2.05 は合格ライン 2.5 にそこそこ近いので、**wall が 0.2s 単位で効くタスク
（A-5, A-9, B-8 など warm 系）ではさらに静かな状態を作ること。**

### 1.3 現在の phase 内訳（**2026-07-27 クリーン再計測**。ここが「攻める場所」の地図）

**cold（空 `GUFF_CACHE`, `--no-cache`）— wall 4.71〜4.79s / RSS 7.60〜7.72GB / findings 20**

| phase | wall | 何をしているか | 攻めるタスク |
|---|---:|---|---|
| load_graph（`go list`） | **1.25s** | 外部プロセス待ち。CPU は遊んでいる | B-8 / C-3（warm は B-8） |
| typecheck_roots | **1.87s** | うち seed 1.42s（1455 依存パッケージ）/ target 0.43s | P0-2, P0-3, A-2〜A-4, B-5, B-6 |
| analyze | **1.22s** | 全 analyzer の実行 | B-0〜B-4 |
| format_checks | **1.75s**（並列に重畳、待ち 0.00s） | 2 スレッド専用プールで実行。**記録値 0.59s の 3 倍** | **P0-1（最優先）** |
| issues+filter | **0.03s** | もう十分速い | 触らない |
| print | 0.00s | | 触らない |

**cold seed-hot（`GUFF_CACHE` 永続, `--no-cache`）— wall 3.68s / RSS 7.31GB**

| phase | wall |
|---|---:|
| load_graph | 1.22s |
| typecheck_roots | 0.88s（seed dep check **0.39s**、1455 hit / 0 miss） |
| analyze | 1.19s |
| format_checks | **1.90s**（重畳、待ち 0.00s） |

> ⚠️ **seed-hot では format_checks（1.90s）が wall（3.68s）の半分を超えています。**
> cold を速くすればするほど format の比率が上がり、いずれ critical path になります。
> P0-1 を最優先にしている理由がこれです。

**warm（issues+fmt キャッシュ hot）— wall 0.35〜0.36s / RSS 0.14GB**

| phase | wall |
|---|---:|
| load_graph（`go list`） | **0.21〜0.22s**（＝ warm wall の **60%**） |
| cache setup+partition | 0.09s（294 hit / 0 miss） |
| typecheck_roots | 0.01s（0 pkgs） |
| analyze | 0.00s |
| issues+filter | 0.03s |
| format_checks | 0.09〜0.10s（fmt_check キャッシュ hot） |

warm を詰めるなら **`go list` の 0.21s（B-8）が主戦場**です。ここを潰さない限り warm は
0.2s を割れません。**逆に P0-1 は warm には効きません**（warm の format は 0.09s）。

### 1.4 analyze の中身（**2026-07-27 クリーン実測**、worker 横断の合計 CPU）

**この表は wall ではありません**（`PERF_TASKS.md` §1.6 の注意を再読すること）。
並列に走るので実 wall はこの 1/6 くらいです（実際 analyze の wall は 1.20s）。

```
                         buildir      2.34s      66 actions   ← 1 pkg あたり 35ms
                     testifylint      1.15s      19 actions   ← 1 pkg あたり 61ms
                          revive      0.78s     293 actions
                        misspell      0.43s     293 actions
                       modernize      0.30s     293 actions
                          inline      0.28s     293 actions
                      whitespace      0.25s     293 actions
                       copylocks      0.19s     293 actions
                       typeindex      0.19s     293 actions
                        gocritic      0.12s      66 actions
                          SA5001      0.09s     293 actions
                       structtag      0.09s     293 actions
                      composites      0.09s     293 actions
                        errorsas      0.08s     141 actions
                           godot      0.08s      66 actions
                          SA1012      0.08s     116 actions
                     unreachable      0.08s     293 actions
                          SA4023      0.07s      66 actions
                    unusedresult      0.07s     293 actions
                          ST1005      0.07s     293 actions
（上位 20 の合計 ≈ 6.7s。以下に 200 個以上の analyzer の裾野が続く）
```

読み取れること:

- **buildir（SSA 構築）が単独で 35%。** 66 actions しかない＝1 パッケージあたり **35ms**。→ **B-2**
- **testifylint が 19 actions で 1.15s ＝ 1 パッケージあたり 61ms**。単価は buildir の
  1.7 倍で、全 analyzer 中で最悪。→ **B-3**
- 上位 2 つで **52%**。3 位以下は「293 actions で 0.07〜0.78s」＝1 パッケージ 0.2〜2.7ms の
  小物が延々と並ぶ。個々は安いが **数が 200 以上あるので裾野の合計が効く**。
  この裾野の大半は「AST を丸ごと 1 周舐めて、欲しいノード種別以外を捨てる」だけに
  費やされている。→ **B-0 / B-1**

> 参考: 汚染環境（§1.1）で同じ表を取ると全部が 2.2 倍（buildir 5.19s / testifylint 2.80s）に
> なります。**比率は変わらない**ので、汚染環境でも「どれが重いか」の判断には使えますが、
> **GO/NO-GO の絶対値の判定には使えません。** 必ずクリーン環境で取り直すこと。

---

## 2. 検証プロトコル（毎タスク必須。省略したらそのタスクは失敗）

### 2.1 findings 同一性

`PERF_TASKS.md` §2.1 の `gen()` 関数をそのまま使ってください。**diff が空でなければロールバック。**
件数の一致で満足しないこと（§2.1 の注意書き参照）。

### 2.2 決定性

`PERF_TASKS.md` §2.2。**通常 3 回、ハッシャ/並列/キャッシュを触るタスクは 5 回。**

### 2.3 本番ゲート

```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff
./regress/run.sh                 # tsdb プロファイル（軽い。まずこれ。~1.7s の計測 + golangci-lint）
./regress/run.sh --profile full  # ./... 全体（本番。重い）
```

合格条件（`regress/gate.py`）:

| 項目 | しきい値 |
|---|---|
| wall | baseline × **1.0** + **0.15s**（第1弾の記述より厳しくなっています。実測値を信じること） |
| peak RSS | baseline × **1.20** |
| `guff_only` の増分 | **0** |
| `golangci_only` の増分 | **0** |
| `both` の減少 | **禁止** |

現 baseline（`regress/baseline*.json`）と、**2026-07-27 クリーン環境での実測**:

| プロファイル | baseline wall | 実測 wall | baseline RSS | 実測 RSS | findings | 判定 |
|---|---:|---:|---:|---:|---|---|
| tsdb | 1.740s | **1.630s** | 1,319,845,888 | **1,293,205,504** | both 4 / only 0,0 | **PASS** |
| full | 4.940s | **4.770s** | 7,608,352,768 | **7,570,423,808** | both 20 / only 0,0 | **PASS** |

**つまり第2弾の着手時点で、両プロファイルとも green です。**
最初のタスクに入る前にこの表を再現できることを確認してください。再現できないなら、
それは**あなたの変更のせいではなく環境のせい**である可能性が高い（→ §1.1）。

**両プロファイル PASS しなければコミットしない。baseline はユーザー承認まで更新しない**（§0-6）。

> ⚠️ wall のしきい値が `ratio=1.0, epsilon=0.15s` と非常に厳しいので、**§1.2 のガードを通していない
> 汚れた環境ではゲートがほぼ確実に落ちます。** ゲートが落ちたときは、まず「マシンが汚れていないか」
> を疑ってから、コードを疑ってください。

### 2.4 `-j 1` でも遅くなっていないこと

`PERF_TASKS.md` §0-3。並列パスにだけ重い処理を足していないかの確認です。

```bash
cd .../prometheus
CACHE=$(mktemp -d); GUFF_CACHE="$CACHE" /usr/bin/time -lp "$GUFFBIN" run --no-cache -j 1 -c .golangci.yml ./... >/dev/null; rm -rf "$CACHE"
```

---

## 3. タスク一覧（全 33 本）

**リスク**は「findings を壊す確率」、**工数**は実装＋検証の総量です。
**期待**は cold wall（prometheus `./...`）に対するクリーン環境での見込み削減量。
数字に `?` が付いているものは **GO/NO-GO 計測をしないと分からない**という意味です。

### Tier S — 計測基盤（**他のどのタスクより先にやる**）

| ID | タスク | 効く所 | 期待 | 工数 | リスク |
|---|---|---|---|---|---|
| S-1 | 計測環境クリーン判定スクリプト | （全部） | — | 極小 | ゼロ |
| S-2 | プロファイラ（samply）手順の常設 | （全部） | — | 小 | ゼロ |
| S-3 | `GUFF_DEBUG_CACHE=2` で phase 内訳を細分化 | （全部） | — | 小 | 低 |

### Tier P0 — 今回の調査で見つかった「たぶん既に損している」もの

| ID | タスク | 効く所 | 期待 | 工数 | リスク |
|---|---|---|---|---|---|
| **P0-1** | **format_checks の 2 スレッド固定を見直す** | cold | **0.3〜0.8s?** | 小 | 低 |
| **P0-2** | **依存パースで `SKIP_OBJECT_RESOLUTION`** | cold | **0.2〜0.5s?** | 小 | 低 |
| P0-3 | target パースの object resolution も条件スキップ | cold | 0.05〜0.15s? | 中 | 中 |

### Tier A — 低リスク・小〜中の確実な勝ち

| ID | タスク | 効く所 | 期待 | 工数 | リスク |
|---|---|---|---|---|---|
| A-1 | ハッシャを FxHash に差し替え | cold/warm | 0.1〜0.4s? | 中 | **中**（§0-12） |
| A-2 | Scanner の `src.to_vec()` 除去 | cold | 0.05〜0.15s? | 小 | 低 |
| A-3 | トークンごとの `String` 割り当て削減 | cold | 0.1〜0.3s? | 中 | 低 |
| A-4 | `File::add_line` の Mutex 除去 | cold | 0.05〜0.2s? | 小 | 中 |
| A-5 | `hex_encode` の `format!` 除去 | warm | 0.01〜0.05s? | 極小 | 極低 |
| A-6 | `Context::lookup` の `Vec` 割り当て除去 | cold | 0.0〜0.1s? | 小 | 低 |
| A-7 | `typecheck_one_target` の `Package` 丸ごと clone 除去 | cold | 0.0〜0.1s? | 小 | 中 |
| A-8 | `target-cpu=native` / PGO ビルド | 全部 | 0.2〜0.7s? | 中 | 低 |
| A-9 | 起動コスト（レジストリ構築・設定パース）の計測と削減 | warm | 0.0〜0.05s? | 小 | 低 |

### Tier B — 構造的（中〜大。必ず GO/NO-GO 計測を先に）

| ID | タスク | 効く所 | 期待 | 工数 | リスク |
|---|---|---|---|---|---|
| ~~B-0~~ | ~~preorder 総時間の計測（B-1 の GO/NO-GO）~~ **DONE** | — | 27.9% と判明 | 小 | ゼロ |
| **B-1** | **本物の Inspector（フラットイベント列 + 種別マスク）** | cold | **0.15〜0.25s**（B-0 実測に基づく改訂。上限 0.32s） | 大 | 中 |
| B-2 | buildir/SSA の関数単位 遅延構築 | cold | 0.2〜0.5s? | 大 | **高** |
| B-3 | testifylint の高速化（単価 61ms の解明） | cold | 0.1〜0.3s? | 中 | 中 |
| B-4 | revive の `shared_walk` を全ルールに拡大 | cold | 0.05〜0.2s? | 中 | 中 |
| B-5 | 型の構造的インターン（hash-consing） | cold | 0.1〜0.3s? | 大 | **高** |
| B-6 | 型チェッカの `Expr::clone` 除去 | cold | 0.1〜0.3s? | 中 | 中 |
| B-7 | ソースバイトの一回読みを format/misspell と共有 | cold | 0.05〜0.2s? | 中 | 低 |
| B-8 | warm の `go list` をパース済み形式でキャッシュ | **warm** | **0.1〜0.18s?** | 中 | 中 |
| B-9 | seed の wave バリアを部分的に撤廃 | cold | ~0.1s | 大 | **高** |

### Tier C — 大物・実験（時間と注意力に余裕があるときだけ）

| ID | タスク | 効く所 | 期待 | 工数 | リスク |
|---|---|---|---|---|---|
| C-1 | AST アリーナ化 + 文字列インターン | cold | 0.3〜1.0s? | **特大** | **最高** |
| C-2 | 常駐デーモン / watch モード | warm | 0.41s→~0.05s | **特大** | 中 |
| C-3 | `go list` の自前置き換え | cold/warm | ~1.3s | **特大** | **最高** |
| C-4 | gocritic 106 チェッカーの walk 融合 | cold | 0.0〜0.1s? | 大 | 中 |
| C-5 | issue cache を analyzer 単位の粒度に | warm | ? | 大 | **高** |
| C-6 | `Ident` から `Mutex` を外す | cold | 0.05〜0.2s? | 中 | 中 |
| C-7 | 依存 seed のプリウォーム（バックグラウンド投機実行） | cold | ? | 大 | 中 |
| C-8 | メモリ削減（7.6GB → 4GB）で並列度を上げる | cold | ? | 大 | 中 |

**推奨着手順:** `S-1 → S-2 → S-3 → P0-1 → P0-2 → A-5 → A-2 → B-0 → （B-0 の結果次第で B-1）→ A-1 → …`

自信がなければ **Tier S と Tier P0 だけやって終わりにしてよい**です。それでも十分な成果です。

---

## 4. タスクテンプレート（コピーして使う）

各タスクに着手したら、これを作業メモに貼って埋めていってください。**全部 YES になるまでコミット禁止。**

```
## タスク: <ID> <名前>
- [ ] `docs/PERF_TASKS.md` §0 の10ルール + このファイル §0 の5ルールを読み直した
- [ ] §1.2 のガードを走らせ、マシンがクリーンであることを確認した
      load avg = ____ / ncpu = ____ / lowpowermode = ____
- [ ] GO/NO-GO 計測をやり、削減上限 = ____ s と見積もった（0.1s 未満なら中止）
- [ ] 変更前の findings を取った（/tmp/before.txt、件数 ____）
- [ ] 実装した（触ったファイル: ____）
- [ ] `cargo build --release` が警告増なしで通る
- [ ] `cargo test --workspace` が通る
- [ ] 変更後の findings を取った（/tmp/after.txt）
- [ ] `diff /tmp/before.txt /tmp/after.txt` が **空**
- [ ] 同条件 3 回（ハッシャ/並列/キャッシュ系は 5 回）で after.txt が毎回同一
- [ ] 狙った phase の秒数が実際に下がった: ____ s → ____ s
- [ ] `-j 1` でも遅くなっていない: ____ s → ____ s
- [ ] RSS が baseline × 1.20 以内: ____ bytes
- [ ] `./regress/run.sh` PASS
- [ ] `./regress/run.sh --profile full` PASS
- [ ] baseline は更新していない
- [ ] このコミットに他タスクを混ぜていない
- [ ] このファイル（PERF_TASKS_V2.md）の該当タスクに DONE と実測値を追記した
```

---

# Tier S — 計測基盤

## S-1 — 計測環境クリーン判定スクリプト

### 目的

§1.1 の事故（Chrome に 2 コア食われて全 phase 2.2 倍）を二度と起こさないため、
計測前チェックを 1 コマンドにする。

### 手順

`scripts/perf-guard.sh` を新規作成する。仕様:

1. `sysctl -n hw.ncpu` でコア数を取る（Linux なら `nproc`）。
2. `uptime` から 1 分 load average を取る。**load > ncpu/4 なら FAIL。**
3. `ps aux | sort -nrk3` の上位 10 件を出す。**自分（guff / cargo / rustc）以外で
   CPU 20% 超のプロセスがあれば WARN、50% 超なら FAIL。**
4. macOS なら `pmset -g | grep lowpowermode` が 0 でなければ FAIL、
   `pmset -g therm` に警告があれば FAIL。
5. `cargo` / `rustc` / `cursor-agent worker` / `go build` が走っていれば FAIL。
6. FAIL なら exit 1 して、**何が原因かと対処法**（「Chrome を閉じる」等）を出す。
7. `--wait` オプションで、クリーンになるまで 5 秒おきにリトライ（最大 5 分）できるようにする。

そのうえで `regress/run.sh` の冒頭（`ROOT=` を決めた直後あたり）から呼び出し、
**`PERF_GUARD=0` で無効化できる**ようにする（CI や意図的な計測で邪魔になるため）。

### 検証

- 汚れた状態（Chrome で YouTube を再生する等）で走らせて FAIL すること。
- クリーンな状態で PASS すること。
- `PERF_GUARD=0 ./regress/run.sh` が従来どおり動くこと。
- **guff 本体のコードは1行も触らないので findings 検証は不要**（ただし `regress/run.sh` を
  触るので、`./regress/run.sh` が最後まで走ることは確認する）。

### やってはいけない

- guff のバイナリ側に判定を入れる（ユーザーの実行時に load average を見に行くのは筋が悪い）。
- FAIL でいきなり `pkill` する。**警告して止まるだけ**にする。

---

## S-2 — プロファイラ（samply）手順の常設

### 目的

今のところ「phase タイマー」しか計測手段がない。関数レベルでどこに時間が行っているかを
見られないと、Tier A/B のタスクは当てずっぽうになる（§0-14 に反する）。

### 背景

このリポジトリには現在プロファイラが入っていません（`which samply cargo-flamegraph` → 両方なし）。
`samply` は macOS / Linux 両対応で、Firefox Profiler の UI で読めるサンプリングプロファイラです。
コード変更が要らず、release バイナリにそのまま使えます。

### 手順

1. インストール:
   ```bash
   cargo install samply
   ```
2. **`strip = true` を一時的に外す必要があります。** ルートの `Cargo.toml` の
   `[profile.release]` に `strip = true` があるとシンボルが消えてプロファイルが読めません。
   プロファイル専用プロファイルを追加するのが安全:
   ```toml
   # ルート Cargo.toml に追記
   [profile.profiling]
   inherits = "release"
   strip = false
   debug = 1
   ```
   ビルド: `cargo build --profile profiling`（成果物は `target/profiling/guff`）。
   **`[profile.release]` 自体は絶対に変更しないこと**（`PERF_TASKS.md` のコメントにあるとおり
   `lto`/`codegen-units`/`panic` は意図して選ばれている）。
3. 取得:
   ```bash
   cd .../prometheus
   CACHE=$(mktemp -d)
   GUFF_CACHE="$CACHE" samply record -- \
     ../target/profiling/guff run --no-cache -c .golangci.yml ./... >/dev/null
   rm -rf "$CACHE"
   ```
4. 手順を `docs/DEVELOPMENT.md` のプロファイリング節に追記する。

### 注意

- **`profiling` プロファイルで測った wall を regress のゲートに使ってはいけません。**
  `strip=false` / `debug=1` はバイナリサイズを変えるので、数字が `release` と一致しません。
  ゲートは常に `target/release/guff`。
- 並列実行なので、フレームグラフは worker スレッドごとに分けて見ること。
  「合計 CPU」と「wall」を混同しない（§1.6 の再掲）。

### 検証

コード変更なし。`cargo build --release` が従来どおり通ること、
`./regress/run.sh` が PASS することだけ確認。

---

## S-3 — `GUFF_DEBUG_CACHE=2` で phase 内訳を細分化

### 目的

現在の phase タイマーは粒度が粗すぎて、Tier A/B の GO/NO-GO 判定に使えません。
たとえば `load_graph 1.53s` の内訳（サブプロセス待ち / JSON パース / グラフ構築）が分からない。

### 手順

1. 既存の `timing_enabled()` 相当（`GUFF_DEBUG_CACHE` の有無を見ている箇所）を探す:
   ```bash
   rg -n 'GUFF_DEBUG_CACHE' crates --glob '*.rs'
   ```
2. **既存の `=1` の出力は 1 行も変えない**まま、`GUFF_DEBUG_CACHE=2`（＝値が `2` 以上）のときだけ
   追加で出る詳細レベルを足す。
3. 追加してほしい内訳:
   - `load_graph` の中: `go list` サブプロセスの実時間 / stdout バイト数 / JSON パース /
     `connect_imports` / `refine`。（`crates/guff-packages/src/golist.rs` に
     `golist invoke(main)` / `golist parse+build` が既にあるので、それを `=2` に載せ替える）
   - `typecheck_roots target check` の中: ファイル読み / パース / 型チェックの内訳（合計 CPU で可）
   - `analyze` の中: **`InspectResult::preorder` に費やした総 CPU**（→ B-0 で使う）
   - `format_checks` の中: ファイル収集 / formatter ごとの内訳
4. 出力形式は既存に合わせる（`guff:   <2段インデント><名前> <秒>s`）。

### やってはいけない

- 計測コードをホットループの内側に置いて、それ自体が遅くする。
  `Instant::now()` はナノ秒オーダーだが、1000万回呼べば効く。
  **ループの外で `Instant` を取り、スレッドローカルに累算して最後に集計**する。
  （既存の `record_analyzer_time` / `ANALYZER_TIMING`（`crates/guff-runner/src/action.rs`）が
  グローバル `Mutex<HashMap>` を使っているが、これは analyzer 1回につき1回なので OK。
  preorder のような高頻度のものに同じ実装を使ってはいけない。）
- `GUFF_DEBUG_CACHE` が未設定のときにオーバーヘッドを足す。**必ず `bool` を一度読んで使い回す。**

### 検証

- `GUFF_DEBUG_CACHE=1` の出力が**変更前とバイト同一**であること（`diff` で確認）。
- `GUFF_DEBUG_CACHE` 未設定での wall が変わらないこと（3 回計測して誤差内）。
- findings diff = 空、両 regress PASS。

---

# Tier P0 — たぶん既に損しているもの

## P0-1 — `format_checks` の 2 スレッド固定を見直す（**最優先**）

### 目的

format_checks が **2 スレッドの専用 rayon プールに固定**されており、実測で **5.0s** かかっています。
コードのコメントは「2 workers は `go list`（~1.3s）の間に終わるのに十分」と書いていますが、
**実測は 5.0s で、前提が成り立っていません。** その間ずっと 10 コアのうち 2 コアを
typecheck/analyze から奪い続けています。

### 現状の証拠

`crates/guff-lint/src/lib.rs:640-671`:

```rust
fn run_and_write_inner(opts: &LintOptions, out: &mut dyn Write) -> Result<i32, RunError> {
    // ...
                // Format checks use rayon heavily; pin them to a small private
                // pool so they don't steal workers from the global pool that
                // typecheck/analyze need during the overlap window.
                // 2 workers is enough to finish during `go list` (~1.3s) without
                // starving analysis; 1 worker makes format the critical path.
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(2)
```

**実測（2026-07-27、クリーン環境で 3 回。§1.1 の対照実験済み）:**

```
cold（空 GUFF_CACHE）      : format_checks 1.70 / 1.79 / 1.78s   （wall 4.75s）
cold seed-hot（永続 CACHE）: format_checks 1.90s                 （wall 3.68s）
warm（キャッシュ hot）      : format_checks 0.09〜0.10s           （wall 0.36s）
```

**これは汚染では説明できません。** §1.1 のとおり、クリーン環境では他の全 phase が
`PERF_TASKS.md` §1.10 の記録値とぴったり一致しています（load 1.53→1.25s、
typecheck 1.79→1.87s、analyze 1.16→1.22s）。**format_checks だけが 0.59s → 1.75s と
3 倍**残りました。

内訳の説明はつきます。`PERF_TASKS.md` §1.10 が記録している 0.59s は
**まだグローバルプール（全コア）で走っていた頃の値**なので、
`0.59s × 有効並列度 6 ≈ 3.5 CPU 秒 ÷ 2 スレッド ≈ 1.75s`。ぴったり合います。
**つまりバグではなく、意図した隔離の副作用です。**

問題は、コメントが根拠にしている「2 workers は `go list`（~1.3s）の間に終わる」が
**事実として成り立っていない**ことです。実際には 1.75s かかり、`go list` が終わったあとも
0.5s ぶん typecheck/analyze と 2 コアを取り合っています。
**そしてより深刻なのは seed-hot のケース**で、format 1.90s / wall 3.68s ＝ **wall の 52%** です。
cold が速くなるほど比率が上がり、いずれ critical path になります。

### GO/NO-GO 判定

**判定済み: GO。** 上記の実測（クリーン環境・3 回・対照実験つき）がそのまま根拠です。
再測して `format_checks` が 1.5s を大きく超えることだけ確認してから着手してください。

```bash
cd .../prometheus
GUFFBIN=.../target/release/guff
CACHE=$(mktemp -d)
GUFF_CACHE="$CACHE" GUFF_DEBUG_CACHE=1 /usr/bin/time -lp \
  "$GUFFBIN" run --no-cache -c .golangci.yml ./... >/dev/null 2>/tmp/f.txt
grep -E 'phase |^real' /tmp/f.txt
rm -rf "$CACHE"
```

1.0s 未満に下がっていたら（＝誰かが既に直したなら）NO-GO。

### 手順

1. スレッド数を**環境変数で差し替えられる**ようにする（例 `GUFF_FMT_THREADS`、
   未設定なら現状の 2）。これは恒久 API ではなく**実験用のつまみ**なので、
   `README` には載せず、コード内コメントに「実験用」と明記する。
2. `GUFF_FMT_THREADS` を **1 / 2 / 3 / 4 / ncpu/2 / ncpu** で cold を各 3 回計測し、
   **wall（＝ format 単体の秒数ではなく全体の wall）** が最小になる値を探す。
   表にして記録すること。
3. **重要: 見るべきは format_checks の秒数ではなく wall です。**
   format が速くなっても typecheck/analyze が遅くなれば意味がありません。両方の秒数を記録する。
4. 最適値が 2 でなければ、デフォルトを最適値に変える。
   ただし**ハードコードした定数ではなく `available_parallelism()` からの相対値**にする
   （10 コア機に最適化した数字が 4 コア機で破滅しないように）。
   例: `max(2, ncpu / 4)`。
5. さらに踏み込むなら: **format のワークを 1 個ずつ global pool に投げる**方式も比較する。
   rayon の `spawn` で低優先度に流せば、専用プール自体が要らなくなる可能性があります。
   ただしこれは work-stealing の挙動が読みにくいので、**まずは 1〜4 のスレッド数チューニングだけ**で
   区切ってコミットしてください（§0-9: 1タスク1論点）。

### 検証

- findings diff = 空（format の findings は "File is not properly formatted" 行なので、
  スレッド数で変わったら並列バグです。**変わったら即ロールバックして原因を探す**）。
- 3 回決定性。
- **wall が下がったこと**（format_checks 単体ではなく wall）。
- `-j 1` でも遅くなっていないこと。**`-j 1` のときは format も 1 スレッドにすべきか**を
  併せて検討する（`opts.sequential` のときの挙動を確認）。
- RSS が増えていないこと（スレッドを増やすと同時に開くファイルバッファが増える）。
- 両 regress PASS。

### やってはいけない

- `--fix` の逐次パスを並列化する。**`--fix` は解析が読んでいるファイルを書き換える**ので、
  逐次であることが正しさの前提です（コードコメント参照）。
- スレッド数を `ncpu` にして「format は速くなりました」で終わる。wall で判断すること。

### ロールバック基準

wall が下がらない / findings が変わる / `-j 1` が遅くなる → `git checkout -- crates/guff-lint/src/lib.rs`。

### DONE（2026-07-27）— **結論: デフォルトは 2 のまま（wall には効かない）。実験用つまみだけ追加。**

`GUFF_FMT_THREADS`（実験用・README 非掲載）を `fmt_thread_count()`
（`crates/guff-lint/src/lib.rs`）に追加し、cold full `./...` を 1/2/3/4/5/10 スレッドで
各 3 回スイープした（クリーン環境・§1.2 ガード合格・sweep スクリプトは settle 待ち込み）:

| threads | wall (中央値) | format_checks | typecheck_roots | analyze |
|---:|---:|---:|---:|---:|
| 1 | 4.61s | 3.53s | 1.84s | 1.18s |
| **2（既定）** | **4.50s** | 1.62s | 1.75s | 1.14s |
| 3 | 4.54s | 1.29s | 1.75s | 1.14s |
| 4 | 4.73s | 0.77s | 1.76s | 1.15s |
| 5 | 4.75s | 0.68s | 1.75s | 1.15s |
| 10 | 4.80s | 0.58s | 1.75s | 1.14s |

**判明したこと: format_checks は分析ウィンドウ（load_graph 1.2s + typecheck 1.75s + analyze
1.15s ≈ 4.1s）に完全に重畳しており、`waited=0.00s`。つまり critical path に乗っていない。**
スレッドを増やすと format 単体は速くなる（1.62→0.58s）が、**wall はむしろ悪化**する
（分析と CPU を取り合うだけ）。**wall の最小は 2 スレッド（4.50s）。**

**seed-hot（永続 GUFF_CACHE, --no-cache）でも同じ**: 2 スレッド wall 3.46s / 3 スレッド 3.53s。
typecheck が 0.83s に縮んでも、load_graph+analyze が残るので format（1.8s）は依然重畳しきる。
doc が懸念した「format が wall の 52% ＝ critical path 化」は**測定上は起きていない**
（52% は重畳ぶんであって、直列に足されているわけではない）。

したがって **§0-14 / ルール14 に従い、削減上限 ≈ 0s なのでデフォルト変更は NO-GO**。
当初検討した `(ncpu/4).max(2)` は、このスイープでは 12 コア以上の機で 3〜4 スレッドになり
**wall を悪化させる**ため採用しない。`GUFF_FMT_THREADS` だけ残し（他ハードでの再測定用）、
既定は定数 2。findings byte 一致（20/20）、tsdb gate PASS、`-j 1` 挙動も不変（既定を触っていない）。

---

## P0-2 — 依存パースで `SKIP_OBJECT_RESOLUTION` を使う

### 目的

guff の parser は、パース後に **Go の `ast.Object` 解決（`resolve_file`）** を走らせています。
これは「識別子から宣言へのポインタを AST に埋める」処理で、Go 本体でも **deprecated**
（`parser.SkipObjectResolution` で切れる）です。

**seed 用の依存パース（1455 パッケージ / 約 12318 ファイル）では、この結果を誰も読んでいません。**
型チェッカ（`guff-types`）は独自のスコープ解決をするので `ast.Object` を一切参照しません。
つまり丸ごと無駄です。

### 現状の証拠

モードは既に存在します:

`crates/guff-ast/src/parser.rs:62`:

```rust
pub const SKIP_OBJECT_RESOLUTION: Mode = Mode(1 << 6);
```

`crates/guff-ast/src/parser.rs:3150-3151`:

```rust
        if !mode.contains(SKIP_OBJECT_RESOLUTION) {
            resolve_file(&mut file, &pos_file, None);
```

しかし依存パースは指定していません:

`crates/guff-packages/src/typecheck.rs:1036`:

```rust
        if let Ok(file) = parse_file(fset, name, src, SKIP_FUNC_BODIES) {
```

そして `guff-types` / `guff-analysis` / `guff-packages` は `ident.obj` を**一度も読みません**。
自分で確認してください:

```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff
rg -n 'obj\.lock' crates/guff-types crates/guff-analysis crates/guff-packages
# → 0 件であることを確認する
```

`ast::Ident.obj` を読んでいるのは以下の 3 箇所だけです（**依存パースの経路には居ません**）:

```bash
rg -n 'obj\.lock' crates --glob '*.rs' | grep -v '^crates/guff-ast/'
# crates/guff-ineffassign/src/cfg.rs:766   ← target パッケージのみ（P0-3 の対象）
# crates/guff-style/src/maintidx.rs:202    ← target パッケージのみ（P0-3 の対象）
# crates/guff-fmt/src/native/goimports/fix.rs:152 ← 自前で別に parse している。無関係
```

### GO/NO-GO 判定

`resolve_file` にどれだけ時間が使われているか、単発で測ってください。
S-3 で `=2` の詳細タイマーを入れているなら、`parse_dep_sources` の中で
`parse_file` の合計 CPU を出し、`SKIP_OBJECT_RESOLUTION` を付けた版と付けない版で比べます。

手っ取り早くやるなら、`crates/guff-packages/tests/` か一時的なバイナリで、
依存クロージャの .go を全部読んで両モードでパースし、シングルスレッドで秒数を比べる
（`PERF_TASKS.md` §1.9 の末尾がまさにこの手法を使っています）。

**合計 CPU の削減が 0.5s 未満なら NO-GO**（seed は既に wall 1.4s まで縮んでおり、
有効並列度 6 で割ると wall 0.08s 未満にしかならないため）。

### 手順

1. `crates/guff-packages/src/typecheck.rs:1036` を
   `parse_file(fset, name, src, SKIP_FUNC_BODIES | SKIP_OBJECT_RESOLUTION)` にする
   （`Mode` の `|` 実装があることを確認。無ければ `Mode(SKIP_FUNC_BODIES.0 | SKIP_OBJECT_RESOLUTION.0)`
   相当を使う。`rg -n 'impl.*BitOr.*Mode' crates/guff-ast/src/parser.rs` で確認）。
2. インポートを追加する。
3. **`parse_dep_sources` の doc コメントを更新する。** 既に `SKIP_FUNC_BODIES` について
   「seed は exported API しか要らない」と書いてあるので、そこに object resolution の話も足す。
   `PERF_TASKS.md` §1.9 のコメントと同じ密度で書くこと。

### 検証

- **findings diff = 空**（これが本丸。依存の `ast.Object` が本当に未使用なら 1 バイトも変わらない）。
- 3 回決定性。
- **seed キャッシュとの相互作用に注意**: seed の永続キャッシュ（`${GUFF_CACHE}/seed/`）は
  「パース結果」ではなく「型チェック済み overlay」を保存しているので、キーは変わらないはず。
  ただし念のため **seed キャッシュを空にした cold と、seed hot の cold で findings が同一**である
  ことを確認すること（`PERF_TASKS.md` Task 4 の検証項目と同じ）。
- `seed dep check` の秒数が下がったこと。
- RSS はむしろ下がるはず（`Arc<Object>` を作らないぶん）。**増えていたらおかしい。**
- 両 regress PASS。

### やってはいけない

- **`crates/guff-packages/src/typecheck.rs:417`（target パッケージのパース）を同時に変える。**
  そちらは `ineffassign` と `maintidx` が `ident.obj` を読むので、findings が変わります。
  分けて P0-3 でやってください（§0-9: 1タスク1論点）。
- 「たぶん誰も使ってない」で確認せずに進める。**上の `rg` を自分の手で走らせて 0 件を確認すること。**

### ロールバック基準

findings が 1 件でも変わったら即ロールバック。その場合、依存パースの `ast.Object` を
どこかが読んでいるということなので、**それを特定してからこのファイルに追記**してください
（次のエージェントが同じ罠を踏まないように）。

### DONE（2026-07-27）— **GO。cold wall −0.13s、seed dep check −0.2s、findings byte 一致。**

`crates/guff-packages/src/typecheck.rs:1036`（`parse_dep_sources`）を
`SKIP_FUNC_BODIES | SKIP_OBJECT_RESOLUTION` にした。import に `SKIP_OBJECT_RESOLUTION` を追加、
doc コメントに「型チェッカも analyzer も dep の `Ident.obj` を読まない（唯一の読者
ineffassign/maintidx は target 側）」旨を追記。

**事前確認（doc 指定の rg を実行済み）:**
`rg 'obj\.lock' crates/guff-types crates/guff-analysis crates/guff-packages` → **0 件**。
guff-ast 外の `obj.lock` は 3 箇所のみ＝ goimports/fix.rs（自前 parse・無関係）、
ineffassign/cfg.rs・maintidx/maintidx.rs（**target 専用**・P0-3 の対象）。依存パース経路には居ない。

**実測（クリーン環境・§1.2 ガード合格）:**

| 指標 | before | after |
|---|---:|---:|
| seed dep check（wave-parallel, 1455 deps） | 1.33〜1.39s | **1.16〜1.17s** |
| cold full wall | 4.50s | **4.36〜4.37s** |
| full regress wall | 4.94s (baseline) | **4.28s** PASS |
| full regress peak RSS | 7.608GB (baseline) | **7.577GB**（低下・doc 予想どおり） |

**検証: findings byte 一致 20/20（cold）、決定性 3/3 一致、seed-hot↔cold 一致、
`-j 1` findings 一致（wall 9.70s、変更は仕事を減らすだけなので遅くなりようがない）、
tsdb+full 両 regress PASS（guff_only=0 / golangci_only=0 / both 減少なし）。**
RSS は `Arc<Object>` を作らないぶんむしろ下がった。baseline 未更新。

---

## P0-3 — target パースの object resolution も条件付きスキップ

> **前提: P0-2 を完了・コミットしてから着手すること。**

### 目的

P0-2 で依存パース（1455 pkg）を潰したら、次は target パース（293 pkg、ただし**関数本体つきの
フルパース**なので 1 ファイルあたりのコストは依存より高い）。

### 難所

target では `ident.obj` を読む利用者が 2 つあります:

- `crates/guff-ineffassign/src/cfg.rs:766` — `id.obj.lock().unwrap().is_none()` で
  「`false` / `nil` がシャドウされていない組み込み識別子か」を判定している
- `crates/guff-style/src/maintidx.rs:202` — `id.obj.lock().unwrap().clone()`

つまり **`ineffassign` と `maintidx` のどちらかが有効なら resolution が必要**です。
prometheus の `.golangci.yml` で両方が有効かどうかを先に確認してください:

```bash
cd .../prometheus && cat .golangci.yml
```

### 手順

1. `LintOptions` から「有効な analyzer 名の集合」を取り、
   `ineffassign` / `maintidx` のどちらかが含まれるかを判定する述語を作る。
   **判定は analyzer 名の集合照合だけ**（型情報もパス情報も不要・O(1)）。
2. その述語が偽なら、target パースにも `SKIP_OBJECT_RESOLUTION` を付ける。
3. 述語の結果を `TypecheckEnv` 相当に載せて `typecheck.rs:417` まで運ぶ。
   **グローバル変数や環境変数で運ばないこと**（テストが並列で走ると壊れる）。

### 検証（P0-2 より厳しく）

- **`ineffassign` / `maintidx` を有効にした構成**と**無効にした構成**の両方で findings を取り、
  それぞれ変更前とバイト同一であること。**片方だけ確認して終わりにしない。**
  ```bash
  # 無効側の確認例（一時的な設定ファイルを作る）
  # 有効側は prometheus の .golangci.yml をそのまま使う
  ```
- `ineffassign` が実際に検出を出すコードで、検出が消えていないことを名指しで確認する。
  prometheus に無ければ、小さな fixture を作って手で確認する。
- 3 回決定性、両 regress PASS。

### やってはいけない

- 「prometheus の設定では ineffassign が無効だから常にスキップでいい」と決め打つ。
  **guff は他人の設定でも動かなければいけません。** 必ず条件判定にする。
- 述語を「型チェック後」に評価する。パース前に決まっていなければ意味がありません。

---

# Tier A — 低リスク・確実な勝ち

## A-1 — ハッシャを FxHash に差し替え

### 目的

このリポジトリは **`HashMap` を全部 `std` のデフォルト（SipHash 1-3 + `RandomState`）で使っています。**
FxHash / ahash などの高速非暗号ハッシャは入っていません:

```bash
rg -n 'rustc-hash|fxhash|ahash' Cargo.lock   # → hashbrown 以外ヒットなし
```

SipHash は DoS 耐性のために選ばれていますが、guff は**信頼できるローカルのソースコードしか
読まない**ので、その耐性は不要です。短いキー（識別子・パッケージパス）では FxHash が 2〜5 倍速いです。

### 対象（優先順）

ホットな順に。**一度に全部やらないこと。** 1 crate ずつ、1 コミットずつ。

1. `crates/guff-types/src/scope.rs` の `Scope.elems: HashMap<String, ObjectId>`
   — スコープ検索は型チェックの最内周
2. `crates/guff-types/src/check.rs` の `import_cache` / `obj_map` / `methods` / `untyped`
3. `crates/guff-types/src/api.rs` の `Info.types` / `defs` / `uses`（ノード id → 型。巨大）
4. `crates/guff-ast/src/token.rs` の `keywords()`（`&'static str` キー。頻度は最高）
5. `crates/guff-runner/src/cache.rs` / `action.rs`

### 事前に必ずやること（§0-12）

**イテレーション順依存の監査。** これを飛ばすと非決定性を埋め込みます。

```bash
cd /Users/dakimura/projects/src/github.com/dakimura/guff
# 対象 crate に絞って、HashMap を直接 iterate している箇所を洗う
rg -n 'for .* in .*(\.iter\(\)|\.values\(\)|\.keys\(\)|\.drain\(\))' crates/guff-types/src
```

見つかった各箇所について「順序が結果に影響するか」を判断し、影響するなら
**先にそこを直す（別コミット）**。典型的には `collect()` して `sort_by_key()` するか、
`BTreeMap` にする。`Scope.elems` は `Serialize`/`Deserialize` も付いているので、
**シリアライズ順が変わると seed キャッシュのバイト列が変わる**点にも注意（後述）。

### 手順

1. ワークスペースの `Cargo.toml` に依存を足す:
   ```toml
   # ルート Cargo.toml の [workspace.dependencies] があればそこ、無ければ各 crate に
   rustc-hash = "2"
   ```
   （`rustc-hash` v2 は `FxHashMap` / `FxHashSet` を提供します。バージョンは
   `cargo add rustc-hash` で最新を取ること。**バージョンをでっち上げない。**）
2. 1 crate ずつ、`use std::collections::HashMap` を `use rustc_hash::FxHashMap as HashMap` に
   置き換える。型引数の数が違う（`HashMap<K, V>` のままでよい）ので機械的に済みます。
3. **`Serialize`/`Deserialize` が付いている構造体のマップは要注意。**
   `Scope.elems` は seed キャッシュに bincode でシリアライズされます。
   マップのシリアライズ順が変わると **同じ内容でもバイト列が変わる**ので、
   `SEED_OVERLAY_SCHEMA`（`crates/guff-types/src/check.rs`）を**インクリメントする必要があるか**を
   検討してください。厳密には内容が同じならデコード結果も同じなので必須ではありませんが、
   古いキャッシュとの互換性を疑うなら上げるのが安全です。判断に迷ったら**上げる**。

### 検証（5 回決定性が必須）

- findings diff = 空。
- **§2.2 を 5 回**（ハッシャ変更は順序依存バグを叩き起こす筆頭）。
- **seed キャッシュ空 cold ↔ seed hot cold で findings 同一。**
- **`-j 1` と `-j N` の両方で findings 同一。**
- 狙った phase（typecheck_roots）が下がったこと。
- 両 regress PASS。

### やってはいけない

- 全 crate を一括置換して 1 コミットにする。**壊れたとき切り分け不能。**
- `HashSet` を見落とす（`FxHashSet` も同様に置き換える）。
- 監査を飛ばす。

### ロールバック基準

findings が 1 回でも揺れたら即ロールバック。**「5 回中 4 回同じだった」は失敗です。**

---

## A-2 — Scanner の `src.to_vec()` を除去

### 目的

スキャナは初期化時にソース全体を**コピー**しています:

`crates/guff-ast/src/scanner.rs:144`:

```rust
        self.src = src.to_vec();
```

prometheus の依存クロージャは約 **150MB / 12318 ファイル**（`PERF_TASKS.md` §1.9 の実測）。
つまり cold 1 回につき 150MB の無駄な memcpy と、その分のアロケータ負荷・ページフォルトが
発生しています。

### GO/NO-GO 判定

150MB の memcpy 自体は数十 ms（メモリ帯域律速）なので、**単体では小さい**です。
効くのはむしろ「アロケータ経由の 12318 回の大きな確保・解放」とキャッシュ汚染のほう。
S-3 か samply（S-2）で `Scanner::init` / `to_vec` の比率を先に見てください。
**wall 換算 0.05s 未満なら NO-GO。**

### 手順

`Scanner` にライフタイムを付けて借用にするのが本筋ですが、`Scanner` が構造体フィールドとして
`Parser` に持たれているので、ライフタイムが波及します。波及範囲を先に確認:

```bash
rg -n 'struct Scanner|Scanner \{|scanner:' crates/guff-ast/src | head -20
```

波及が大きすぎるなら、**`Arc<[u8]>` に変える**のが折衷案です。呼び出し側が
`Arc<[u8]>` を持っていれば `Arc::clone` で済み、コピーが消えます。
`parse_file` はソースを `&[u8]` で受け取っているので、`parse_file` 側で 1 回だけ
`Arc<[u8]>` 化する（＝コピー 1 回は残るが、`Scanner` 内のコピーは消える）。

**より良い案:** `parse_dep_sources`（`crates/guff-packages/src/typecheck.rs`）は
ファイルを自分で読んで `&[u8]` を渡しているので、**そこで `Arc<[u8]>` を作って
`parse_file` に渡せばコピーはゼロになります。** `parse_file` に `Arc<[u8]>` を
受ける新 API（例 `parse_file_shared`）を足し、既存の `parse_file` はそれを呼ぶ薄い
ラッパにするのが後方互換で安全です。

### 検証

- findings diff = 空、3 回決定性。
- `cargo test --workspace`（`guff-ast` のテストが多いので必ず通す）。
- **RSS を必ず確認。** `Arc<[u8]>` にすると、パース中はソースが生き続けます。
  今は `to_vec` したコピーがスキャナと一緒に落ちる設計なので、**共有にするとむしろ
  RSS が増える可能性があります。** ここが最大の落とし穴。増えたら NO-GO。
- 両 regress PASS。

### やってはいけない

- ソースを `String` に変える。`from_utf8_lossy` の挙動（不正 UTF-8 の扱い）が変わって
  findings が変わります。**`[u8]` のまま**扱うこと。

---

## A-3 — トークンごとの `String` 割り当てを削減

### 目的

スキャナは**トークン 1 個につき `String` を 1 個**作ります:

`crates/guff-ast/src/scanner.rs:750-752`:

```rust
    pub fn scan(&mut self) -> (Pos, Token, String) {
```

識別子・数値・文字列・ルーン・コメント、さらに挿入セミコロンの `"\n".to_string()` まで。
`PERF_TASKS.md` §1.9 の実測では依存クロージャで **2230 万トークン**。
そのうち大半はキーワード・区切り記号で、**リテラル文字列が要らない**トークンです。

### 段階（この順に。1 段ずつコミット）

**A-3a（最小・最安全）: 空文字列の割り当てを消す。**
記号・キーワードのトークンは `lit` が使われません。`String::new()` は実はアロケートしないので
既にゼロコストですが、`";".to_string()` / `"\n".to_string()` は**アロケートします**。
これを `&'static str` を返す形にするか、`Cow<'static, str>` にする。
呼び出し側（`parser.rs:233-238`）が `self.lit = lit` しているだけなので影響は局所的。

**A-3b: 識別子を `Box<str>` にする。**
`String` は 24 バイト（ptr/len/cap）、`Box<str>` は 16 バイト（ptr/len）。
`Ident.name` は AST に大量に残るので、`Box<str>` 化はメモリ削減にも効きます。
ただし `name: String` を前提にしたコードが多いので、影響範囲を先に測ること:
```bash
rg -c '\.name\b' crates --glob '*.rs' | sort -t: -k2 -nr | head
```

**A-3c（本命だが大きい）: 識別子のインターン。**
Go のソースは同じ識別子（`err`, `ctx`, `nil`, `String`, `Context` …）が何万回も出ます。
グローバルな `Symbol` テーブル（`u32` ハンドル）にすれば、比較が整数比較になり、
メモリも激減します。**ただしこれは C-1 の一部**なので、単体でやるなら
「スキャナ内の一時バッファ再利用」までに留めるのが安全です。

### GO/NO-GO 判定

samply（S-2）で `alloc` / `malloc` / `free` のサンプル比率を見てください。
**アロケータが総サンプルの 10% を超えていなければ、A-3b/c は NO-GO。**
A-3a は工数が極小なので、GO/NO-GO なしでやってよいです。

### 検証

- findings diff = 空、3 回決定性。
- `cargo test --workspace`（スキャナのテストが厚いので、ここで壊れたら即分かる）。
- **RSS が下がっていること**（下がらないなら効いていない）。
- 両 regress PASS。

### やってはいけない

- `from_utf8_lossy` を `from_utf8_unchecked` に変える。**不正 UTF-8 のファイルで UB。**
  Go の scanner は不正 UTF-8 を明示的にエラーにするので、挙動を変えると findings が変わります。

---

## A-4 — `File::add_line` の Mutex を除去

### 目的

スキャナは**改行 1 個ごとに Mutex を取ります**:

`crates/guff-ast/src/position.rs:173-180`:

```rust
    pub fn add_line(&self, offset: i64) {
        let mut m = self.mutable.lock().unwrap();
        // ...
        m.lines.push(offset);
    }
```

呼び出し元は `Scanner::next`（`crates/guff-ast/src/scanner.rs:173-176`）。
150MB のソースなら数百万回のロック取得です。**無競合の Mutex は約 20ns** なので
数百万回で 0.05〜0.1s、しかも並列パース中は同一 `FileSet` 上で
（別ファイルなら別 `File` なので実際の競合は少ないはずですが）アトミック命令が走ります。

### 手順

**1 ファイルのパースは 1 スレッドで完結する**ので、パース中はロック不要です。

1. `Scanner` にローカルの `line_offsets: Vec<i64>` を持たせ、改行のたびにそこへ push する。
2. パース完了時（`parse_file` の最後、`Scanner` を落とす直前）に、**1 回だけロックを取って
   まとめて `File` に流し込む**。
3. **ただし途中で `File` の行テーブルを読む人が居ないかを必ず確認すること。**
   ```bash
   rg -n 'fn line\b|fn position\b|fn offset\b|line_count|\.lines\b' crates/guff-ast/src/position.rs
   rg -n '\.position\(|\.line\(' crates/guff-ast/src/parser.rs crates/guff-ast/src/scanner.rs
   ```
   **パース中にエラーを報告する経路（`error()` / `Bailout`）が行番号を引いている可能性が高い**ので、
   そこは要注意。引いているなら、ローカル `Vec` からも引けるようにするか、
   その経路だけ従来どおりにする。

### 検証

- findings diff = 空。**特にパースエラーを含むファイル**（`compat/` や
  `regress/fmt_fixtures/` に無ければ、わざと壊した .go を一時的に作る）で
  エラーメッセージの行番号が変わらないことを確認する。
- 3 回決定性、`cargo test --workspace`、両 regress PASS。

### やってはいけない

- `Mutex` を `RwLock` に変えるだけ。**書き込みが主なので改善しません。**
- `unsafe` でロックを外す。

---

## A-5 — `hex_encode` の `format!` を除去（**一番簡単。腕慣らしに最適**）

### 目的

キャッシュキーの 16 進エンコードが**バイトごとに `format!`** しています:

```bash
rg -n 'fn hex_encode' -A6 crates/guff-runner/src/cache.rs
```

`format!("{b:02x}")` は 1 回で `String` を確保し、フォーマット機構を通します。
SHA-256 は 32 バイトなので 1 ハッシュにつき 32 回。
パッケージ 1792 個 × ファイル数ぶん呼ばれます。

### 手順

ルックアップテーブル方式に置き換える:

```rust
const HEX: &[u8; 16] = b"0123456789abcdef";

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}
```

**出力は 1 バイトも変わらないこと**（小文字 16 進・ゼロ埋め 2 桁）を確認してください。
`format!("{b:02x}")` は小文字ゼロ埋め 2 桁なので上と同一です。

### 検証

- **既存のユニットテストがあるか確認し、無ければ書く**（`assert_eq!(hex_encode(&[0x00, 0x0f, 0xff]), "000fff")`）。
- findings diff = 空（キャッシュキーが変わるとキャッシュが全ミスするので、
  **キーのバイト列が同一であること**が本質）。
- **warm を 2 回まわして、2 回目がちゃんと hit すること**を `GUFF_DEBUG_CACHE=1` の
  `hits=` で確認する。ここが変わったらエンコードが違っています。
- 両 regress PASS。

### 発展（別タスクにする）

SHA-256 自体を **blake3** に置き換えると 5〜10 倍速くなります。ただし
**キャッシュキーが全部変わる**ので既存キャッシュが全ミスします。
やるなら「キャッシュのスキーマバージョンを上げる」とセットで、別コミットで。
効くのは cold の `set_dep_hashes`（全パッケージの全ファイルを SHA する）なので、
**先に S-3 でそこの秒数を測ってから**判断してください。

### DONE（2026-07-27）— **findings byte 一致・cache 全 hit 維持。腕慣らしどおり安全。**

`crates/guff-runner/src/cache.rs:848` の `hex_encode` を、`fmt_cache.rs` に既にある
LUT 実装（`b"0123456789abcdef"`）に置き換え。`format!("{b:02x}")` と**バイト同一**（小文字・
ゼロ埋め 2 桁）。ユニットテスト `hex_encode_matches_format` を追加し、空 / `0x00` / `0x0f` /
`0xff` / 混在 / **全 256 バイト**で新旧が一致することを検証（`cargo test -p guff-runner`）。

**検証:** findings byte 一致 20/20（cold）、決定性 3/3、warm 2 回目が `hits=294 misses=0`
（キャッシュキーのバイト列が変わっていない＝エンコード同一の証拠）、両 regress PASS。

> 補足: `crates/guff-fmt/src/fmt_cache.rs` の `hex_encode` は既に LUT 化済みだったので、
> 残っていた `format!` 版は `cache.rs` の 1 箇所だけ。期待どおり cold wall への寄与は
> 測定ノイズ以下（この関数はハッシュ確定ごとに 1 回・SHA-256 32 バイト）だが、
> **腕慣らし＆一貫性**目的なので DONE 判定は findings 同一と cache hit 維持で足りる。

---

## A-6 — `Context::lookup` の `Vec` 割り当てを除去

### 目的

ジェネリクスのインスタンス化キャッシュが、**参照するたびに `Vec` を確保**しています:

```bash
rg -n 'fn lookup' -A4 crates/guff-types/src/context.rs
```

`self.instances.get(&(orig, targs.to_vec()))` — キーが `(TypeId, Vec<TypeId>)` なので、
**読むだけなのに `targs` をコピー**しています。

### 手順

`hashbrown` の raw entry API か、キーをハッシュ可能な借用形に変えるのが定石です。
一番簡単なのは **キーを事前にハッシュした `u64` にする**方式:

1. `(orig, targs)` から決定的なハッシュ（例 FNV-1a / blake3 の先頭 8 バイト）を計算する関数を作る。
2. `instances: HashMap<u64, TypeId>` にする。
3. **ハッシュ衝突が起きたら型を取り違えます。** これは findings を壊すので、
   衝突対策として `HashMap<u64, Vec<(TypeId, Vec<TypeId>, TypeId)>>`（バケット内で厳密比較）に
   するか、64bit で衝突を許容しない設計にする。**手抜きすると静かに壊れます。**

より安全な代替: `targs` が短い（普通 1〜2 個）ので **`SmallVec<[TypeId; 4]>`** にすれば
ヒープ確保がゼロになります。`smallvec` クレートを足すだけで、ロジックは一切変わりません。
**こちらを推奨します。**

### GO/NO-GO 判定

ジェネリクスを多用しないコードベースでは、そもそも `lookup` がほとんど呼ばれません。
prometheus がどれだけジェネリクスを使っているか不明なので、
**まず `lookup` の呼び出し回数をカウンタで数えてください。100 万回未満なら NO-GO。**

### 検証

- findings diff = 空、3 回決定性。**ジェネリクスを含むパッケージで検出が変わらないこと**を重点確認。
- `cargo test --workspace`（`guff-types` のジェネリクステストが本丸）。
- 両 regress PASS。

---

## A-7 — `typecheck_one_target` の `Package` 丸ごと clone を除去

### 目的

`crates/guff-packages/src/typecheck.rs:285`:

```rust
    let mut pkg = (**by_id.get(id)?).clone();
```

`Package` は `Vec<PathBuf>`（compiled_go_files）、import マップ、各種メタデータを持つ
そこそこ大きい構造体です。それを **293 パッケージぶん丸ごと deep clone** しています。

### 手順

1. **なぜ clone しているのかを先に読む。** おそらく「型チェック結果（syntax / types /
   types_info）を書き込むために所有権が要る」からです。`Arc<Package>` を共有したまま
   結果だけ別に持てないか検討する。
2. 最小の改善は「clone せず、必要なフィールドだけ move / 参照する」。
   `Arc::try_unwrap` が使えるなら（他に参照が無ければ）コピーゼロで所有権が取れます。
3. **`by_id` に `Arc` が残っている限り `try_unwrap` は失敗します**ので、
   `by_id` の構築方法から見直す必要があるかもしれません。

### GO/NO-GO 判定

293 回の clone です。**1 回 1ms でも 0.3s** ですが、実際は数十 µs でしょう（0.01s 程度）。
samply で `Package::clone` が見えなければ **NO-GO**。
このタスクは「見た目が悪いだけで実は安い」可能性が高いです。

### やってはいけない

- **`Package` の中に pointer identity を map のキーにしているものがないか確認せずに触る**
  （`PERF_TASKS.md` §0-8）。
  ```bash
  rg -n 'as \*const .* as usize|ptr::eq' crates/guff-packages/src crates/guff-types/src
  ```

---

## A-8 — `target-cpu=native` / PGO ビルド

### 目的

現在 `.cargo/config.toml` が存在せず、`RUSTFLAGS` も設定されていません
（`ls .cargo` → 無し）。つまり **generic な arm64/x86-64 向けコードが出ています。**

### 段階

**A-8a: `target-cpu=native`（ローカル計測用のみ）**

```toml
# .cargo/config.toml（新規）
# 注意: これはローカル開発機向け。配布バイナリはこれを使ってはいけない（他マシンで
# 不正命令になる）。CI / リリースビルドでは無効化すること。
[build]
rustflags = ["-C", "target-cpu=native"]
```

⚠️ **これを入れると配布バイナリが壊れる可能性があります。** リリース CI が
`cargo build --release` を素で叩いているなら、`.cargo/config.toml` はリポジトリに
**コミットしてはいけません**（`.gitignore` に入れる）。
`.github/workflows/` と `Dockerfile` を必ず確認してから決めること:

```bash
cat .github/workflows/*.yml | grep -n 'cargo build'
grep -n 'cargo build' Dockerfile
```

**判断: リポジトリにコミットするなら、`[target.<host-triple>]` 節に限定するか、
環境変数 `RUSTFLAGS` で都度渡す運用にする。** 一番安全なのは
「`docs/DEVELOPMENT.md` に `RUSTFLAGS='-C target-cpu=native' cargo build --release` と書くだけ」です。

**A-8b: PGO（Profile-Guided Optimization）**

これは**確実に効く**（rustc 自身が PGO で 10〜20% 速くなっています）が、ビルド手順が複雑です。

```bash
# 1) 計装ビルド
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release
# 2) 代表的なワークロードを走らせる（prometheus cold + warm 両方）
cd prometheus && ../target/release/guff run --no-cache -c .golangci.yml ./... >/dev/null
# 3) プロファイルをマージ（llvm-profdata が要る）
xcrun llvm-profdata merge -o /tmp/pgo-data/merged.profdata /tmp/pgo-data
# 4) 本ビルド
RUSTFLAGS="-Cprofile-use=/tmp/pgo-data/merged.profdata" cargo build --release
```

これを `scripts/build-pgo.sh` として常設し、`docs/DEVELOPMENT.md` に書く。

### 検証

- **findings diff = 空**（最適化フラグで findings が変わったら、それは
  未定義動作か浮動小数点の非決定性があるということ。**その場合は最適化を諦めるのではなく
  バグを探すこと**）。
- 3 回決定性。
- wall が下がったこと。
- **両 regress PASS。ただし baseline との比較は「同じビルド設定で取った baseline」と
  やらないと意味がありません。** PGO ビルドの速さを baseline に焼き込むと、
  以後の通常ビルドが全部 FAIL します。**PGO は「ローカルで速くする手段」であって
  「baseline を更新する理由」ではありません。**（§0-6 の精神）

### やってはいけない

- PGO ビルドの数字で `--update-baseline` する。**絶対にやらないこと。**
- `.cargo/config.toml` を CI 確認なしにコミットする。

---

## A-9 — 起動コスト（レジストリ構築・設定パース）の計測と削減

### 目的

warm 実行は **wall 0.41s** しかないので、プロセス起動〜レジストリ構築のコストが
無視できない比率になっている可能性があります。しかし**現在それを測る phase タイマーがありません**
（最初の phase は `load_graph`）。

`crates/guff-lint/src/registry.rs` は linter 名から analyzer 集合を解決しますが、
staticcheck だけで **161 個**、style で **77 個**、govet で **30 個** の
`Analyzer` 構造体（`requires: Vec<...>`, `fact_types: Vec<...>` を持つ）を組み立てます。

### 手順

1. **まず測る。** `main()` の先頭で `Instant::now()` を取り、
   `run_linters` に入る直前までを `GUFF_DEBUG_CACHE=1` で
   `phase startup (config+registry) X.XXs` として出す。
2. **0.02s 未満なら NO-GO。** そのまま計測コードだけコミットして終わり（それでも価値があります）。
3. 0.05s を超えるようなら:
   - `OnceLock` で全 analyzer を毎回構築しているなら、**実際に有効な linter のぶんだけ**
     構築するように遅延化する。
   - `Vec<&'static Analyzer>` の重複除去（`registry.rs` の `partition_linters` 周辺）が
     O(n²) になっていないか確認する。
   - 設定 YAML のパースが重いなら、`serde_yaml` の使い方を見直す。

### 検証

- findings diff = 空、3 回決定性。
- **warm の wall が下がったこと**（cold では誤差に埋もれます。`PERF_TASKS.md` §1.4 の warm 手順で測る）。
- 両 regress PASS。

---

# Tier B — 構造的（GO/NO-GO 計測を必ず先に）

## B-0 — preorder 総時間の計測（**B-1 の GO/NO-GO。単独で完結するタスク**）

### 目的

B-1（本物の Inspector）は工数「大」なので、**やる価値があるかを先に数字で確かめます。**
このタスクは計測だけで、最適化は一切しません。**結果が NO-GO でも成功です。**

### 背景（なぜ怪しいか）

`InspectResult` は**空の構造体**で、`preorder()` は呼ばれるたびに AST を丸ごと歩き直します:

`crates/guff-analysis/src/passes/inspect.rs:13-36`:

```rust
/// Empty on purpose: this port rewalks on each [`preorder`] call, so collecting
/// node ids at analyzer-run time was unused overhead.
#[derive(Clone, Default)]
pub struct InspectResult {}

impl InspectResult {
    pub fn preorder<F>(&self, files: &[File], mut f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        let mut stack = Vec::new();
        for file in files {
            preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                f(n);
                true
            });
        }
    }
}
```

呼び出し箇所は **144 箇所**（`rg -n '\.preorder\(' crates | wc -l`）。
そしてそのほとんどが、**最初の 1 行で欲しいノード種別以外を捨てています**:

`crates/guff-govet/src/assign.rs:39-42`:

```rust
    inspect.preorder(pass.files(), |n| {
        let NodeRef::AssignStmt(AssignStmt { tok, lhs, rhs, .. }) = n else {
            return;
        };
```

`crates/guff-staticcheck/src/sa4006.rs:119-122`:

```rust
    inspect.preorder(pass.files(), |node| {
        let NodeRef::ForStmt(fs) = node else {
            return;
        };
```

つまり **`AssignStmt` が欲しいだけの analyzer が、全ノード（数万個）を訪問して 99% を捨てている。**
これが analyzer の数だけ繰り返されます。

**Go 本体はこれを解決済みです。** `golang.org/x/tools/go/ast/inspector.Inspector` は
AST を**一度だけ**歩いてフラットなイベント配列（各要素にノード種別のタグ付き）を作り、
`Preorder(types ...ast.Node)` はビットマスクで**該当種別のイベントだけを線形スキャン**します。
さらに「この種別が部分木に存在しない」なら部分木ごとスキップできます。
**これは §0-13 の「参照実装にある最適化」の教科書的な例です。**

### 手順（計測のみ）

1. `InspectResult::preorder` に**スレッドローカルの累算カウンタ**を仕込む:
   - 呼び出し回数
   - 訪問ノード総数
   - 費やした総ナノ秒
   - できれば「呼び出し元 analyzer 名」ごとの内訳（`Pass` から analyzer 名が取れるなら）
2. **グローバル `Mutex` を使わないこと。** `thread_local!` + `Cell<u64>` で累算し、
   スレッド終了時 or 解析終了時に集計する。ここは超高頻度パスです。
3. `GUFF_DEBUG_CACHE=1`（または S-3 の `=2`）で以下を出す:
   ```
   guff: inspect preorder: 1234 calls, 45,678,901 nodes visited, 3.21s total CPU
   ```
4. cold の prometheus `./...` で計測し、**この表を PERF_TASKS_V2.md のこの節に追記する。**

### GO/NO-GO の判定基準

- 総 CPU が **analyze フェーズの総 CPU（§1.4 の上位20合計 ≈ 6.7s + 裾野）の 20% 未満** なら **NO-GO**。
  B-1 は諦めて、この計測結果を記録して終わり。
- 20〜40% なら **条件付き GO**（B-1 の「軽量版」＝ §B-1 の手順 1〜3 だけ）。
- 40% 超なら **フル GO**。

### 検証

- 計測コードを入れた状態と外した状態で **wall が変わらないこと**（3 回計測して誤差内）。
  変わるなら計測自体が重すぎます。
- findings diff = 空（当たり前ですが確認する）。
- 両 regress PASS。

### やってはいけない

- 計測せずに B-1 を始める（§0-14）。
- `Instant::now()` を `preorder` のコールバック内側（＝ノードごと）に置く。**破滅します。**

### DONE（2026-07-27）— **判定: 条件付き GO（27.9%）。ただし cold wall の上限は 0.32s しかない。**

`InspectResult::preorder`（`crates/guff-analysis/src/passes/inspect.rs`）に
スレッドローカル計装を入れ、`GUFF_DEBUG_CACHE` 有効時のみ集計するようにした。
`crates/guff-runner/src/action.rs` は各アクションの前後でスレッドカウンタを差分して
analyzer 別に按分する（1 アクションは 1 スレッドで完走するので正しい）。

**実測（クリーン環境・cold prometheus `./...`・3 回）:**

```
guff: inspect preorder: 12361 calls, 54385011 nodes visited, 2.11s total CPU (27.9% of analyze CPU)
```

| 回 | preorder 総 CPU | analyze CPU 比 | calls | nodes |
|---:|---:|---:|---:|---:|
| 1 | 2.07s | 27.1% | 12,361 | 54,385,011 |
| 2 | 2.14s | 27.9% | 12,361 | 54,385,011 |
| 3 | 2.11s | 27.9% | 12,361 | 54,385,011 |

calls / nodes は 3 回とも**完全一致**（決定的）。analyze の総 CPU は約 **7.7s**、analyze の
**wall は 1.16s**（有効並列度 ≈ 6.6）。

**判定は §B-0 の基準どおり 20〜40% ＝ 条件付き GO（軽量版＝設計項目 1〜3、部分木スキップ抜き）。
ただし着手前に必ず次の 3 点を読むこと。**

**(1) 上限は 0.32s、現実的には 0.15〜0.25s。** preorder が**タダになっても** analyze wall は
`1.16s × 0.279 = 0.32s` しか縮みません。events 配列の構築コスト（293 pkg × 1 walk）と、
マスクに合致したノードのコールバック本体は残るので、現実的な取り分は **cold wall 0.15〜0.25s**。
doc の当初見積もり「0.3〜0.7s」は**過大**でした。工数「大」＋144 箇所のマスク漏れリスクに
見合うかは、この数字で判断してください。

**(2) analyze CPU の上位は `preorder` を使っていない。** ここが最大の発見です。

| analyzer | analyze CPU | うち preorder |
|---|---:|---:|
| buildir | 2.17s | **0**（SSA 構築。walk しない） |
| testifylint | 1.13s | **0**（独自 walk） |
| revive | 0.73s | **0**（`shared_walk`） |
| misspell | 0.43s | **0** |
| modernize | 0.28s | **0** |
| whitespace | 0.25s | **0** |
| typeindex | 0.17s | **0** |
| gocritic | 0.11s | **0** |
| inline | 0.26s | 0.26s |
| copylocks | 0.20s | 0.19s |
| SA5001 / composites / SA1012 / errorsas / SA4023 / unreachable / structtag / unusedresult / ST1005 … | 各 0.06〜0.10s | ほぼ全部 |

つまり **B-1 が触れるのは analyze CPU 7.7s のうち 2.1s だけ**で、上位 8 個（合計 5.3s）には
1 秒も効きません。§1.4 の「裾野の大半は AST を丸ごと舐めているだけ → B-0/B-1」という読みは
**半分正しく半分間違い**でした（裾野が walk 主体なのは正しいが、裾野の合計が 2.1s しかない）。
逆に言えば、**B-1 を入れたあとに revive / misspell / modernize / whitespace を
同じ inspector に載せ替えれば取り分は倍以上になる**（B-4 がその一部）。
B-1 に着手するなら、この「載せ替え先」もセットで計画すること。

**(3) `preorder` の再帰呼び出し（二次コスト）を計装中に発見。**
`crates/guff-staticcheck/src/sa4023.rs:52`（`interface_from_typed_nil`）は
**preorder のコールバックの中から候補識別子ごとにフルファイル walk を回します**。
その結果 SA4023 だけで **8.4M ノード / 66 アクション**（1 アクション 127k ノード。
inline の 15k の 8 倍）を訪問しています。計装では depth 0 のときだけ時間を積むことで
二重計上を避けています（それをやる前は SA4023 の preorder 時間 0.15s が
自分の analyzer 総時間 0.08s を上回るという矛盾が出ていました）。
**時間としては 0.09s なので単独では追う価値がありませんが、B-1 でマスク版に移行すると
この二次パターンは自動的に安くなります**（内側の walk が `AssignStmt` だけの線形スキャンになる）。

**検証:** findings byte 一致 20/20、tsdb PASS（wall 1.700s / RSS 1.245GB）、
full PASS（wall 4.210s / RSS 7.581GB、guff_only=0 / golangci_only=0 / both 20）。
計装 OFF（`GUFF_DEBUG_CACHE` 未設定）のオーバーヘッドは、変更前バイナリと**交互に 5 往復**
計測して post 中央値 4.35s / pre 中央値 4.36s ＝**差なし**（LazyLock の `bool` を 1 回読む
分岐だけで、ノードごとのコストはゼロ）。baseline 未更新。

> ⚠️ 計測手順の注意: 「変更前を 3 回 → 変更後を 3 回」の順で測ったら +0.27s の差が出ましたが、
> **交互計測ではきれいに消えました**（マシンのドリフト）。0.1s 単位を争うときは
> **必ず交互（A/B/A/B）で測ること。** §1.1 のルール 11 の実運用版です。

---

## B-1 — 本物の Inspector（フラットイベント列 + ノード種別マスク）

> **前提: B-0 を完了し、GO 判定が出ていること。**

### 目的

`x/tools/go/ast/inspector` と同じ設計に置き換え、
**「AST を N 回歩く」を「1 回歩いて N 回フィルタする」**に変える。

### 設計（Go の inspector をそのまま移植する。独自設計をしないこと。§0-13）

Go 側の実装（`golang.org/x/tools/go/ast/inspector/inspector.go`）を読んでから始めてください。
要点だけ:

1. **`events` 配列**を作る。各ノードにつき「push イベント」と「pop イベント」の 2 個。
   各イベントは `{ node, parent_index, typ_bits, index_of_matching_pop }`。
2. **`typ_bits`** はノード種別を表す 1 ビット（56 種類なので `u64` に収まる。
   guff の `NodeRef` はちょうど 56 バリアント — `crates/guff-ast/src/walk.rs` のコメント参照）。
3. `Preorder(mask)` は events を線形に走査し、`typ_bits & mask != 0` のイベントだけ callback する。
4. **最適化の肝:** push イベントに「対応する pop の index」を持たせておくと、
   「この部分木に mask に該当するノードが 1 つも無い」場合に **部分木ごと index をジャンプできる**。
   Go 版はこれで劇的に速くなっています。**この最適化を省くと効果が半減します。**

### 段階（1 段ずつコミット）

**B-1a: イベント配列の構築だけ。** `InspectResult` に events を持たせ、
`preorder()`（マスク無し版）を events の線形走査で実装する。
既存の 144 箇所は**一切変更しない**（API 互換）。
この時点で「N 回の再帰 walk」が「N 回の配列走査」になり、それだけでも
キャッシュ効率で速くなるはずです。**ここで findings 同一・regress PASS を確認してコミット。**

> ⚠️ **メモリに注意。** events 配列は 1 パッケージあたり「ノード数 × 2 × 16〜24 バイト」。
> 大きいパッケージで数 MB になります。293 パッケージが同時に生きると RSS が跳ねます。
> **`release_finished_deps`（`crates/guff-runner/src/action.rs`）が buildir の結果を
> 解放しているのと同じ仕組みで、inspect の結果も最後の消費者が終わったら解放されること**を
> 必ず確認してください。現在 `InspectResult` は空なので解放してもゼロ効果ですが、
> events を持たせた瞬間にここが RSS の生死を分けます。**RSS が baseline × 1.20 を
> 超えたらこのタスクは失敗です。**

**B-1b: マスク付き API を足す。** `preorder_typed(mask, f)` を足し、
`NodeRef` の各バリアントに対応するビット定数を定義する。
既存の `preorder()` は「全ビット立てたマスク」で実装する（互換維持）。

**B-1c: 呼び出し側を 1 つずつ移行する。** 144 箇所を**一度に変えない**。
まず §1.4 の上位（`revive`, `misspell`, `modernize`, `inline`, `copylocks`, `whitespace`）から、
**5〜10 箇所ずつ・1 コミットずつ**移行する。
移行の型は機械的です:

```rust
// before
inspect.preorder(pass.files(), |n| {
    let NodeRef::AssignStmt(a) = n else { return };
    ...
});

// after
inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |n| {
    let NodeRef::AssignStmt(a) = n else { unreachable!() };
    ...
});
```

**`else { return }` を `else { unreachable!() }` に変えるのは危険です**（マスクの指定漏れが
パニックになる）。**`else { return }` のまま残すこと。** 遅くはなりません。

**B-1d: 部分木スキップ最適化を入れる。** B-1a の events に pop index を持たせ、
「部分木に該当ノード無し」でジャンプする。これには**部分木ごとの種別ビット和**を
push イベントに持たせる必要があります（Go 版と同じ）。

### 検証（各段ごとに全部）

- findings diff = 空。**144 箇所の移行では、1 箇所ずつマスクの過不足を疑うこと。**
  マスクが足りないと**検出が消えます**（＝最悪の回帰）。
- **移行した analyzer が実際に検出を出しているパッケージで、検出が減っていないことを名指しで確認する。**
  prometheus の findings は 20 件しかないので、`--profile full` の 20 件だけでは
  マスク漏れを検出できません。**`compat/` のテストスイートも走らせること**:
  ```bash
  ls compat/
  # compat のテスト実行方法を README で確認して走らせる
  ```
- 3 回決定性。
- **RSS を毎段で確認**（上の警告参照）。
- `-j 1` でも遅くなっていないこと。
- 両 regress PASS。

### やってはいけない

- 144 箇所を一括で機械置換する。**マスク漏れが混入したとき、どれが原因か特定できません。**
- `unreachable!()` / `unwrap()` をマスク前提で入れる。
- events を `Vec<Box<dyn ...>>` のような間接参照だらけの構造にする。
  **フラットな `Vec<Event>`（`Event` は `Copy` な小さい構造体）でなければ意味がありません。**
- 部分木スキップ（B-1d）を「難しいから」と省いて B-1 を DONE にする。
  省くなら**明示的に「B-1d 未実施」と本ファイルに書き残す**こと。

### ロールバック基準

findings が変わる / RSS が baseline × 1.20 を超える / wall が下がらない → その段を revert。
**段ごとにコミットしているので、revert は 1 段だけで済みます。これが段階分けの理由です。**

---

## B-2 — buildir/SSA の関数単位 遅延構築

### 目的

buildir は analyzer 総 CPU の **35%**（§1.4）。66 パッケージで 2.34s ＝ 1 パッケージ 35ms。
`PERF_TASKS.md` の「buildir 条件スキップ（Task 5 残り）」は
「staticcheck + nilnesserr が有効だと全 pkg 必須なのでスキップ不可」と結論しています。
**それは正しいですが、「パッケージ単位でスキップできない」だけで、
「関数単位で遅延構築できない」とは言っていません。**

### 背景（先に確認すべきこと）

Go の `x/tools/go/ssa` は `Program.Build()` で全関数を建てますが、
`BuilderMode` に `NaiveForm` などがあり、また staticcheck 本体は
**必要な関数だけ `f.Build()` する**運用をしている箇所があります。
guff の実装がどうなっているか先に読む:

```bash
rg -n 'fn build|src_funcs|Program|BuilderMode' crates/guff-ssa/src/lib.rs | head -30
rg -n 'buildir' crates/guff-analysis/src/passes/buildir.rs
```

### GO/NO-GO 判定

1. **buildir の 35ms/pkg の内訳を samply（S-2）で見る。** 「SSA 命令の生成」なのか
   「型アリーナのクローン」なのかで打ち手が全く違います。
   - 型アリーナのクローンが主なら → **A-1 / B-5 の領域**であって B-2 ではない
   - SSA 生成が主なら → B-2 の GO
2. **buildir の結果を実際に読む analyzer が、全関数を必要としているかを調べる。**
   ```bash
   rg -ln 'buildir::analyzer\(\)' crates/guff-staticcheck/src crates/guff-govet/src | wc -l
   rg -n 'src_funcs' crates/guff-staticcheck/src | head -20
   ```
   ほとんどが `ir.src_funcs` を全部舐めているなら、遅延構築の効果はありません → **NO-GO**。

### 手順（GO の場合）

これは**高リスク・大工数**です。着手する前に、ユーザーに「B-2 に入ってよいか」を確認してください。

1. `Program` の関数を「宣言だけ作って本体は未構築」の状態で持てるようにする。
2. `Function` へのアクセス時に、未構築なら構築する（内部可変性が要る＝**並列アクセスで
   ロックが要る＝競合で逆に遅くなる危険**）。
3. **決定性に最大の注意。** 構築順が実行ごとに変わると、SSA の値 id が変わり、
   それを map のキーにしている箇所（`PERF_TASKS.md` §0-8）が壊れます。
   ```bash
   rg -n 'as \*const .* as usize|ptr::eq|HashMap<.*ValueId' crates/guff-ssa/src crates/guff-staticcheck/src
   ```

### 検証

- findings diff = 空、**5 回決定性**（遅延構築は非決定性の温床）。
- `-j 1` と `-j N` の両方で findings 同一。
- RSS が下がること（建てない関数のぶん）。**増えたら設計が間違っています。**
- 両 regress PASS。

### やってはいけない

- GO/NO-GO を飛ばす。**このタスクは工数が大きく、NO-GO の可能性も十分あります。**
- SSA の構築順を実行ごとに変える設計にする。

---

## B-3 — testifylint の高速化（単価 61ms の解明）

### 目的

§1.4 で **testifylint が 19 actions で 1.15s ＝ 1 パッケージ 61ms** と、
単価が全 analyzer 中で最悪です（buildir の 35ms の 1.7 倍）。
19 パッケージしか走っていないのにこの重さは異常です。

### 手順（まず調査。最適化は後）

1. testifylint の実装を読む:
   ```bash
   rg -ln 'testifylint' crates/*/src
   ```
2. **なぜ 61ms もかかるのかを特定する。** 候補:
   - buildir の結果（`Arc<Program>`）から SSA を舐め直している
   - 正規表現をチェックごとにコンパイルしている
   - AST を何周もしている（→ B-1 で解決）
   - `format!` でメッセージを毎ノード作っている
3. samply（S-2）で testifylint のフレームを見る。**推測で書き換えない。**
4. 原因が分かったら、**このファイルのこの節に「原因は X だった」と追記してから**修正に入る。

### GO/NO-GO 判定

19 actions × 61ms = 1.15s CPU。有効並列度 6 なら wall 換算 **0.19s**。
半分にできれば wall 0.10s。**GO/NO-GO は微妙なライン**なので、
samply で「1 箇所直せば半減する」明確な原因が見えたときだけ着手してください。
ただし「19 パッケージしか走らない」＝ testify を使うパッケージだけ、なので
**他のコードベースでは効果が全く違います。** prometheus に過剰適合しないよう注意。

### 検証

- findings diff = 空。**testifylint の検出が 1 件も消えていないこと**を名指しで確認。
  prometheus で testifylint が何を検出しているかを先にリストアップしておく:
  ```bash
  cd .../prometheus
  "$GUFFBIN" run --no-cache -c .golangci.yml --out-format json ./... 2>/dev/null \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); print("\n".join(sorted(f"{i[\"Pos\"][\"Filename\"]}:{i[\"Pos\"][\"Line\"]}" for i in (d.get("Issues") or []) if i["FromLinter"]=="testifylint")))'
  ```
- 3 回決定性、両 regress PASS。

---

## B-4 — revive の `shared_walk` を全ルールに拡大

### 目的

revive は **293 actions で 0.78s**（3 位）。内部に約 100 個のルールがあり、
そのうち約 50 個は既に `shared_walk`（1 ファイル 1 walk を共有）に統合されています:

```bash
sed -n '1,10p' crates/guff-revive/src/rules/shared_walk.rs
rg -n 'shared_walk' crates/guff-revive/src/rules/mod.rs | head
```

**残り約 50 ルールが個別に walk しています。** これを `shared_walk` に取り込む。

### 手順

1. `crates/guff-revive/src/rules/mod.rs` を読み、`shared_walk` に入っているルールと
   入っていないルールを**リストアップしてこのファイルに書く**。
2. 入っていないルールについて、なぜ入っていないかを 1 つずつ判断する:
   - 単に未対応 → 取り込む
   - walk 以外の情報（型情報・CFG）が要る → 取り込めない。**理由を記録する**
   - 走査順が特殊（後順・部分木限定） → 取り込めない or shared_walk 側の拡張が要る
3. 取り込めるものを **5 ルールずつ・1 コミットずつ**移行する。

### 注意

**B-1（本物の Inspector）が入ると、この作業の前提が変わります。**
B-1 が GO なら、**B-4 より B-1 を先にやってください。** B-1 後は
「revive の各ルールが `preorder_typed` を使う」だけで shared_walk と同等以上になる可能性があります。
（B-1 が NO-GO / 未着手のときだけ B-4 をやる）

### 検証

- findings diff = 空。**revive の検出が 1 件も消えていないこと**を、
  ルール名ごとに確認する（`--out-format json` の `Text` にルール名が入っているはず）。
- 3 回決定性、両 regress PASS。

---

## B-5 — 型の構造的インターン（hash-consing）

### 目的

`*int` や `[]string` のような構造的な型が、**出現するたびに新しい `TypeId` として確保**されています:

```bash
rg -n 'fn new_pointer' -A4 crates/guff-types/src/pointer.rs
```

Go の `types2` も実は毎回作りますが、guff はアリーナが `Vec<TypeData>` なので
**メモリが単調増加**します（RSS 7.6GB の一因の可能性）。

### GO/NO-GO 判定

1. **アリーナに入っている `TypeData` の総数と、重複率を測る。**
   `GUFF_DEBUG_CACHE=2`（S-3）で、解析終了時に `types.len()` と
   「構造的に同一な型の数」を出す。**重複率が 30% 未満なら NO-GO。**
2. RSS の内訳を測る。型アリーナが RSS の主犯でなければ、メモリ動機は消えます。

### 難所（なぜ「高リスク」か）

- **`PERF_TASKS.md` Task 4 の落とし穴節にある「型 identity は構造的（origin ObjectId ベース）で
  instance TypeId ではない」という原理を壊してはいけません。**
  インターンで `TypeId` が共有されると、「`TypeId` が同じ ⇒ 同じ型」だけでなく
  「`TypeId` が違う ⇒ 違う型」も成り立ちそうに見えますが、**成り立ちません**
  （インターンしていない型が残るため）。ここを勘違いしたコードが混入すると壊れます。
- **`Layered` アリーナ（base + overlay）と相性が悪い。** base に既にある型を
  overlay で作り直すのを防ぐには、インターンテーブルも base/overlay に分ける必要があります。
- **並列 wave での seed 構築時、worker ごとに別のインターンテーブルを持つと
  同じ型が worker ごとに別 id になり、merge 後に重複が残ります。**
  つまり「並列だと効きが半減する」。

### 手順（GO の場合。ユーザー確認を取ってから）

1. まず **`TypeArena::alloc` にインターンテーブル（`HashMap<TypeData, TypeId>`）を足すだけ**の
   最小版を作り、**シングルスレッド（`-j 1`）で** 効果を測る。
2. 効果があれば、`Layered` 対応・並列対応に進む。
3. `TypeData` に `Hash` + `Eq` が要ります。**`Named` のような「識別子ベースの同一性」を持つ型は
   インターンしてはいけません。** 構造的な型（Pointer / Slice / Array / Map / Chan / Signature）
   だけを対象にする。

### 検証

- findings diff = 空、**5 回決定性**、`-j 1` と `-j N` の両方。
- **seed キャッシュ空 cold ↔ seed hot cold で findings 同一。**
  インターンは seed の overlay 内容を変えるので、**`SEED_OVERLAY_SCHEMA` を上げる必要が
  ある可能性が高い**です。
- RSS が下がること（これが主目的の一つ。下がらないなら NO-GO）。
- 両 regress PASS。

---

## B-6 — 型チェッカの `Expr::clone` 除去

### 目的

型チェッカが operand に AST 式を**クローンして格納**しています:

```bash
rg -n 'x\.expr = Some\(e\.clone\(\)\)' crates/guff-types/src/expr.rs | head
rg -c 'e\.clone\(\)' crates/guff-types/src/expr.rs
```

`Expr` は 24 バリアントの大きな enum で、`FuncType` / `BlockStmt` / `FieldList` を
インライン保持しています（`crates/guff-ast/src/ast.rs:610-635`）。
つまり **1 回の clone がかなり深いコピー**になります。式のチェックは型チェックの最内周です。

### 手順

1. `Operand.expr` が**何のために必要か**を読む。おそらく「エラーメッセージに式を印字するため」
   だけです。だとすれば **`&Expr` の参照で足りる**はずですが、ライフタイムが波及します。
2. 代替案: **AST ノード id（`u32`）だけを持つ。** `Ident` には既に `id: u32` があり、
   `stamp.rs` が全式に id を振っています（`crates/guff-ast/src/stamp.rs`）。
   id からノードを引く索引が既にあるか確認:
   ```bash
   rg -n 'fn node_by_id|id_to_node|node_index' crates --glob '*.rs'
   ```
   無ければ作るコストと天秤にかけること。
3. さらに軽い代替: **`Arc<Expr>` にする。** AST 側を `Arc` 化する影響が大きいので、
   `Operand` に入れるときだけ `Arc::new(e.clone())`… では意味がありません。**却下。**
4. **一番安全で効果的なのは「エラーが起きたときだけ clone する」** です。
   `x.expr` を `Option<Expr>` から「必要時に呼び出し側が渡す」形に変える。
   エラーは稀（正常なコードでは 0 件）なので、これだけでほぼ全ての clone が消えます。
   **この案を第一候補にしてください。**

### 検証

- findings diff = 空。**型エラーを含むコードでエラーメッセージが変わらないこと**を
  重点確認する（`compat/` のテストか、わざと型エラーのある .go を作る）。
- 3 回決定性、`cargo test --workspace`（`guff-types` のテストが本丸）、両 regress PASS。

---

## B-7 — ソースバイトの一回読みを format / misspell と共有

### 目的

同じ .go ファイルが、1 回の cold 実行で**複数回ディスクから読まれています**:

1. `typecheck.rs` の target パース（`parse_file` 用）
2. `format_checks`（gofumpt / goimports / gci が各々読む → 既に 1 回に共有済み）
3. `misspell`（生のソースバイトを走査。`crates/guff-misspell/src/misspell.rs`）
4. `dupl`（**ディスクから読み直して再パースしている** — `crates/guff-dupl/src/engine.rs:36-46`）
5. `nolint` の事前スキャン（`crates/guff-lint/src/nolint.rs`）
6. generated-file 判定（先頭 16KiB を読む）

`PERF_TASKS.md` の履歴に「typecheck source bytes を misspell/revive/lll/bidichk のために保持」
（コミット `501aabb`）とあるので**一部は共有済み**ですが、format と dupl と nolint は別経路です。

### GO/NO-GO 判定

ページキャッシュが温まっていれば 2 回目以降の read は速い（memcpy 相当）ので、
**効果は「ディスク I/O」ではなく「memcpy とアロケーション」**です。
S-3 でファイル読み込みの総バイト数と総時間を出してから判断してください。
**wall 換算 0.05s 未満なら NO-GO。**

**NO-GO の可能性が高い根拠:** 2026-07-27 のクリーン計測では、cold を 3 回連続で回して
format_checks が 1.70 / 1.79 / 1.78s、wall が 4.75 / 4.71 / 4.79s と**ほぼ完全に一致**しました。
1 回目（＝ページキャッシュが最も冷たいはず）だけが遅いという傾向は**全く見られません**。
つまり**ディスク I/O は既にボトルネックになっていません**（OS のページキャッシュが効いている）。
読み直しのコストは memcpy とアロケーションだけで、これは元々小さい。**まず測って、
それでも 0.05s 以上あるときだけ着手してください。**

### 手順（GO の場合）

1. パッケージ単位で `Arc<[u8]>` のソースキャッシュを持つ構造を作る。
2. **RSS に最大の注意。** 150MB のソースを全部メモリに置き続けたら RSS が増えます。
   **「最後の消費者が終わったら落とす」参照カウント方式**にすること。
3. `dupl` の再パース（`engine.rs:36-46`）は、**そもそも `pass.files()` の AST を
   使えないのか**を先に確認する。使えるなら読み直し自体を消せます（こちらのほうが効く）。

### 検証

- findings diff = 空、3 回決定性。
- **RSS が増えていないこと**（このタスクの最大リスク）。
- 両 regress PASS。

---

## B-8 — warm の `go list` をパース済み形式でキャッシュ

### 目的

warm 実行は **wall 0.36s のうち load_graph が 0.21〜0.22s** ＝ **60%**（2026-07-27 クリーン実測、§1.3）。
残りは cache setup 0.09s / issues+filter 0.03s / format 0.09s で、いずれも既に十分小さい。
**warm を 0.2s 台に乗せる道はここしかありません。**

`${GUFF_CACHE}/golist/` に `go list` の stdout（生 JSON）を保存するキャッシュは既にありますが、
**warm でも毎回 JSON を再パースし、`Package` グラフを再構築しています。**

```bash
rg -n 'golist_cache|try_load_golist_cache|store_golist_cache' crates/guff-packages/src/golist.rs
```

### GO/NO-GO 判定

S-3 で `load_graph` の内訳（サブプロセス / JSON パース / グラフ構築）を warm で出してください。

- サブプロセス起動が主（>0.15s）なら **B-8 は NO-GO**（キャッシュヒット時はサブプロセスを
  起動していないはずなので、この場合はキャッシュが効いていないというバグ）。
- JSON パース + グラフ構築が主なら **GO**。

### 手順（GO の場合）

1. `DriverResponse` / `Vec<Arc<Package>>` を **bincode でシリアライズ**して保存する。
   キーは既存の `golist_cache_key` をそのまま使う（＋スキーマバージョン）。
2. **スキーマバージョン定数を必ず設ける**（`PERF_TASKS.md` Task 4 の教訓）。
   `Package` の構造を変えたら必ずインクリメント。
3. **壊れたキャッシュ / スキーマ不一致は黙って再計算にフォールバック**（クラッシュ厳禁）。
4. `--no-cache` で無効化されること（cold ゲートの前提を壊さない）。

### 検証

- findings diff = 空、**5 回決定性**（キャッシュ系なので）。
- **キャッシュ有無で findings 同一**: `${GUFF_CACHE}/golist/` を消した実行と、hot な実行。
- **キャッシュ無効化が効くこと**: prometheus に .go ファイルを 1 つ足す →
  グラフが変わる → キャッシュミス → 正しく再計算される。（足したファイルを消して元に戻ることも確認）
- **スキーマ不一致 / 破損ファイルでクラッシュしないこと**（ファイルを途中で切って試す）。
- **warm の wall が下がったこと**（`PERF_TASKS.md` §1.4 の warm 手順）。cold では効きません。
- RSS が増えていないこと。
- 両 regress PASS（regress は `--no-cache` なので**この変更の効果は regress には出ません**。
  「悪化していないこと」の確認として走らせる）。

### やってはいけない

- `Package` に `#[derive(Serialize, Deserialize)]` を足すために構造を変える。
- スキーマバージョンを付け忘れる。

---

## B-9 — seed の wave バリアを部分的に撤廃

### 目的と結論（先に読む）

**`PERF_TASKS.md` §1.8 が既に詳細に分析し、「やらない」と結論しています。**

> 非同期化の残り伸び幅は ~0.4s しかないうえ、overlay が base の絶対 id を埋め込む設計上
> マージ順が非決定的になり、seed 永続キャッシュのキー（`base_fp`）が毎回変わって全ミスに
> 落ちる（seed-hot の 0.41s を失う）。よって wave 方式の維持が正解。

**このタスクは原則として着手しないでください。** ここに残してあるのは、
「なぜやらないか」を次のエージェントが再調査しなくて済むようにするためです。

着手してよい唯一の条件: **`base_fp` の設計を変えて、マージ順に依存しない決定的なキーに
できる目処が立った場合。** その場合でも、まずユーザーに相談してください。

---

# Tier C — 大物・実験

以下は**工数が特大**か**リスクが最高**です。時間と注意力に余裕があるときだけ、
かつ**ユーザーに着手の可否を確認してから**始めてください。
各タスクの詳細手順はあえて書いていません（着手が決まった時点で、
そのタスク専用の手順書を書き起こすところから始めるべき規模だからです）。

## C-1 — AST アリーナ化 + 文字列インターン

**要旨:** `Box` / `Vec` / `String` だらけの AST を、アリーナ（`Vec<Node>` + `u32` index）と
シンボルテーブル（`u32` ハンドル）に置き換える。

**期待:** アロケーション激減、キャッシュ効率向上、RSS 大幅減（7.6GB → 4GB 台も狙える）。
**RSS が下がれば並列度を上げられる**ので、二次効果もあります。

**リスク:** **最高。** AST は全 crate が触ります。144 箇所の analyzer が
`NodeRef<'a>` 経由でパターンマッチしているので、そこも全部書き換えになります。

**前提:** A-2 / A-3 / B-1 が全部終わってから。それらを先にやれば、C-1 の効果は目減りします
（＝**C-1 は最後の手段**）。

**着手前に必ず:** 「これをやらないと目標に届かないのか」をユーザーと合意すること。

## C-2 — 常駐デーモン / watch モード

**要旨:** プロセスを常駐させ、パッケージグラフ・型情報・SSA をメモリに保持したまま、
変更されたファイルだけ再解析する（LSP と同じ発想）。

**期待:** warm **0.41s → 0.05s 級**。ruff の `--watch` に相当し、
「ruff 並」を本気で名乗るならここが本丸です。

**リスク:** 中（既存の一発実行パスを壊さずに増設できるため）。ただし**工数が特大**。
インクリメンタル無効化の正しさ（何が変わったら何を捨てるか）が難所で、
ここを間違えると**古い findings を返す**という最悪の回帰になります。

**設計メモ:** 既存の dep-hash レジストリ（`crates/guff-runner/src/cache.rs`）が
すでに「何が変わったか」を決定的に判定できるので、その仕組みをメモリ上で回すのが自然です。

## C-3 — `go list` の自前置き換え

**要旨:** `go.mod` / モジュールキャッシュを自前で読み、`go list` サブプロセスを廃止する。

**期待:** cold 1.3s / warm 0.21s の削減。

**リスク:** **最高。** `PERF_TASKS.md` §0-4 は「`go list` に手を出すな」と明示していますが、
その根拠は「**guff 側のオーバーヘッドがゼロだから**（コストは `go list` 自身）」であって、
「`go list` を置き換えるな」ではありません。とはいえ、build constraints / cgo / vendor /
workspace (`go.work`) / replace directive / ビルドタグの正しい解釈を自前で再実装するのは
**Go ツールチェーンの再実装**であり、パッケージ集合が 1 つでもズレたら findings が変わります。

**現実的な折衷:** 置き換えではなく **`go list` の起動を早める**。
現在は設定パース後に起動していますが、**CLI 引数を読んだ直後（設定ファイルを読む前）**に
投機的に起動できれば、設定パース時間ぶんだけ隠れます。
効果は A-9 で測った起動コストぶんなので、まず A-9 をやってから判断してください。

## C-4 — gocritic 106 チェッカーの walk 融合

**要旨:** `crates/guff-style/src/gocritic.rs`（約 8000 行）は 1 analyzer の中で 106 個の
チェッカーを回し、それぞれが内部で `walk::inspect` を呼んでいます。

**期待:** §1.4 では gocritic は 0.12s / 66 actions と**そこまで重くありません**。
→ **優先度は低い。B-1 が入れば自動的に改善する可能性もあります。**

## C-5 — issue cache を analyzer 単位の粒度に

**要旨:** 現在のキャッシュはパッケージ単位。1 つの analyzer の設定が変わると
そのパッケージ全体が再解析されます。

**リスク:** 高（キャッシュキーの設計変更は `PERF_TASKS.md` Task 2/4 の落とし穴の宝庫）。
**効果が測れていないので、まず「設定変更時の再解析コスト」を測ることから。**

## C-6 — `Ident` から `Mutex` を外す

**要旨:** `ast::Ident` は **識別子 1 個ごとに `Mutex<Option<Arc<Object>>>`** を持っています:

`crates/guff-ast/src/ast.rs:304-316`（`id` の doc コメントは省略）:

```rust
pub struct Ident {
    pub name_pos: Pos,
    pub name: String,
    pub obj: std::sync::Mutex<Option<std::sync::Arc<crate::scope::Object>>>,
    pub id: u32,
}
```

`Mutex` は 8〜16 バイト、`Ident::clone` は**ロックを取ってから**クローンします
（`crates/guff-ast/src/ast.rs:317-325`）。識別子は AST 中で最も数が多いノードです。

**P0-2 / P0-3 が完了して object resolution が実質使われなくなったら、
`Mutex` を `OnceLock` か素の `Option` に落とせる可能性があります。**
（`Mutex` が必要なのは「パース後に後から書き込む」ためなので、
`&mut File` で resolve するように変えれば素の `Option` で済む）

**前提:** P0-2 / P0-3 を先に完了させること。
**期待:** RSS 削減 + clone コスト削減。**A-3b（`Box<str>` 化）と同時にやると効率的**ですが、
§0-9 に従い別コミットにすること。

## C-7 — 依存 seed のプリウォーム（投機実行）

**要旨:** `go list` がサブプロセスで 1.3s 走っている間、コアは遊んでいます
（`PERF_TASKS.md` §1.10 が format_checks をここに重ねたのはこの理由）。
format 以外にも、**前回実行時のパッケージリストを覚えておいて、seed の構築を投機的に始める**
ことができます。

**リスク:** 中。投機が外れたときに無駄な CPU と RSS を使います。
また `PERF_TASKS.md` §0-2（RSS を増やすな）に抵触しやすい。

**前提:** P0-1 の結果次第。もし P0-1 で「format はもっとスレッドを使うべき」と分かったら、
`go list` の間のコアは既に埋まっているので **C-7 は NO-GO** です。

## C-8 — メモリ削減で並列度を上げる

**要旨:** cold の peak RSS が **7.6GB**。これが並列度の上限を決めている可能性があります
（regress の full プロファイルが 18GiB でキルする設定なのはそのため）。
RSS を半減できれば、seed の wave をもっと広く取れる／worker を増やせる可能性があります。

**先にやること:** **RSS の内訳を測る。** 何が 7.6GB を占めているのかが分かっていません。
候補は「型アリーナ」「AST」「SSA (buildir)」「ソースバイト」。
`heaptrack`（Linux）や Instruments の Allocations（macOS）で内訳を取り、
**このファイルに表として記録する**ところまでを 1 タスクにしてください。
それだけで、B-5 / C-1 / C-6 のどれをやるべきかが決まります。

---

## 5. やらないと決めたこと（禁止リスト）

`PERF_TASKS.md` §0 の禁止事項に加えて、今回の調査で「やっても無駄／有害」と判定したもの:

| やらないこと | 理由 |
|---|---|
| `mimalloc` の導入 | **既に入っています**（`crates/guff-lint/src/main.rs:8-9`）。約 19% 速い、と記録あり |
| `lto` / `codegen-units` / `panic` の変更 | ルート `Cargo.toml` のコメントに理由が明記済み。`panic = "abort"` は linter の fault isolation を壊す |
| `issues+filter` の最適化 | 既に 0.04〜0.07s。**第1弾 Task 3 で調査済み** |
| `print` の最適化 | 0.00s |
| seed の parse をさらに詰める | **第1弾 §1.9 末尾で「打ち止め」と結論済み**（残り CPU 0.4s、wall 換算 0.1s 未満） |
| seed の wave バリア撤廃 | B-9 参照。第1弾 §1.8 で分析済み、決定性とキャッシュを失う |
| `GUFF_DEP_SOURCE=0`（export-data 経路） | 第1弾 §0-5。cold 22.9s + 検出数が変わる。袋小路 |
| buildir のパッケージ単位スキップ | 第1弾「buildir 条件スキップ判定」。staticcheck + nilnesserr 有効下では不可 |
| format の shared-read | 第1弾 §1.7。試して**逆に悪化**した |

**新しく「やらない」と判定したものは、必ずこの表に理由つきで追記してください。**
それが次のエージェントの時間を守ります。

---

## 6. コミット前チェックリスト

§4 のタスクテンプレートを使ってください。要点だけ再掲:

- [ ] `PERF_TASKS.md` §0（10ルール）+ 本ファイル §0（5ルール）に違反していない
- [ ] §1.2 のガードでマシンがクリーンなことを確認して計測した
- [ ] GO/NO-GO 計測をやった（削減上限 0.1s 未満なら着手しない）
- [ ] findings diff = 空（**件数一致ではなく diff が空**）
- [ ] 3 回（ハッシャ／並列／キャッシュ系は 5 回）で findings が毎回同一
- [ ] `-j 1` と `-j N` の両方で wall を測り、並列が逐次より遅くなっていない
- [ ] RSS が baseline × 1.20 以内
- [ ] 狙った phase が実際に下がった（下がっていないなら入れる意味がない）
- [ ] `cargo test --workspace` が通る
- [ ] `./regress/run.sh` PASS
- [ ] `./regress/run.sh --profile full` PASS
- [ ] baseline は更新していない（ユーザー承認後のみ）
- [ ] 1 コミット = 1 タスク
- [ ] **本ファイルの該当タスクに DONE と実測値（before → after）を追記した**

---

## 7. ゴールの再設定

`PERF_TASKS.md` §6 は「**warm ≤ 0.4s / cold ≤ 4〜5s**」を現実解としていて、
2026-07-26 時点で **warm 0.39s / cold 4.88s** と、既に達成しています。

第2弾の目標は次のとおりに設定します:

| シナリオ | 現在（**2026-07-27 クリーン実測**） | 第2弾の目標 | 主な手段 |
|---|---:|---:|---|
| cold（空キャッシュ） | **4.75s** | **3.5s** | P0-1, P0-2, B-1, B-3, A-1 |
| cold（seed hot） | **3.68s** | **2.8s** | 同上（seed-hot は特に P0-1 が効く） |
| warm（キャッシュ hot） | **0.36s** | **0.20s** | B-8, A-9 |
| warm（デーモン） | — | **0.05s** | C-2 |
| peak RSS（cold full） | **7.57GB** | **6.0GB** | C-8 の調査結果次第 |

warm の 0.20s は **`go list` の 0.21s（warm wall の 60%）を潰さない限り物理的に不可能**です。
B-8 が warm の唯一の道であり、それ以上を狙うなら C-2（デーモン）しかありません。

**cold の下限は `go list` の 1.3s + 型チェックの実コストで決まる**ので、
C-3 に手を出さない限り 3s を大きく割ることはできません。
**「真の 0.1s」は cold では不可能**という第1弾の結論は変わりません。
そこを狙うなら C-2（デーモン）です。

**そして最後に、第1弾から変わらない最重要の原則:**
**速さより findings 同一が常に優先。findings を守れないなら、その高速化は入れない。**
