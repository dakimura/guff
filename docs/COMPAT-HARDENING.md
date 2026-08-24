# 互換性ハードニング計画（R27）— 唯一の正典

> **このファイルの役割**: golangci-lint との互換性に「自信が持てる」状態に到達するための
> 多セッションにわたる作業計画と進捗記録。**新しいセッションはまずこのファイルを読めば足りる**
> ように書く。個別の設計や実装の一次情報は [`DEVELOPMENT.md`](DEVELOPMENT.md) §8 と
> [`COMPATIBILITY.md`](COMPATIBILITY.md)、ハーネスの使い方は [`../compat/README.md`](../compat/README.md)。
>
> **更新ルール**: フェーズを進めたら §3 の進捗表と §4 のセッションログを必ず更新する。
> 途中で終わったら「次にやること」を具体的なコマンド／ファイル名まで書いて残す。

---

## 1. なぜこの計画が必要か（2026-08-07 時点の実測）

`compat/results/RESULTS.md` と `RESULTS.isolate.md` は全ターゲット `P = R = 100%` を表示している。
しかしその数字が乗っている土台を測ると、**「合格しているが、ほとんど何も比較していない」**状態だった。

| 観測 | 実測値 |
|---|---|
| isolate ゲート（114 linter 全部）が比較している finding | **合計 178 件** |
| うち `both == 0` の空振り合格 | **9 linter**: prealloc, usestdlibvars, maintidx, mirror, musttag, iface, varnamelen, contextcheck, sloglint<br>→ **0 になった**（続き 30。最後の 7 つは fixture がその linter の発火条件を満たしていなかった） |
| うち `both == 1` の 1 件だけ比較 | **72 linter**<br>→ golden 側では 2026-08-24（続き 31）に 18 linter を広げて 75 → 64（linter case で finding 2 件以下のもの） |
| isolate fixture 総行数 | 1,255 行 / 114 linter ≒ **11 行/linter**<br>（gocritic は 13 行で 104 checker、staticcheck は 82 行で 167 analyzer） |
| OSS 8 リポで実際に発火した linter | **7 種類だけ**: errcheck, gosec, govet, ineffassign, modernize, staticcheck, unused<br>（caddy と grafana は `0 vs 0`。436 findings のうち 416 は consul + vault） |
| `crates/*/tests` の 2,848 テスト | `assert!(messages.contains("G101:"))` 形式 = **「guff が撃つこと」の確認**であって<br>**「golangci-lint と同じものを撃つこと」の確認ではない**（ground truth を持たない） |

### 比較キー自体の穴

`compat/normalize.py` の `issue_key()` は `path:line:linter:message`。したがって以下は**構造的に検出不能**:

- **column** — 一切比較していない
- **severity** — 比較していない
- **`--fix` の置換内容（SuggestedFix / Replacement）** — 比較していない
- **staticcheck のチェックコード** — `_STATICCHECK_CODE` が両側から `SA1234: ` を剥がすため、
  guff が `S1003`、golangci が `S1004` と言っていても同じキーになる

さらに `normalize_message()` の 7 種の正規化（errcheck の callee 表示、unused の prefix、
ST1023/QF1011 の言い回し、末尾ピリオド、govet の Go バージョン…）は、
**ユーザーに見える差分を暗黙の allowlist として消している**。

### 方法論は既に実証済み

唯一「真の ground truth と突き合わせた」のが 2026-08 の **gocritic sweep**:
104-checker fixture を golangci-lint 2.12 に実際に食わせてメッセージ単位で差分を取った結果、
**15/156 → 156/156** に跳ね、checker prefix 欠落・`astfmt` ノード描画・ruleguard `$$`・
`Suggest` 文言・報告位置・checker 順序など **12 個の構造的バグ**が出た。

→ **この方法が正しい。産業化されていないだけ。** 本計画は残り 450+ check に同じことを適用する。

---

## 1.5 CI が緑なら何が保証されるのか（2026-08-15）

「CI が通った ＝ 互換性は担保」と言えるようにするための現在地です。
**保証されるものと、CI の外に残っているものを分けて書きます。**

### PR ごとに回るゲート（`compat.yml`）

| ジョブ | 何と比較するか | 保証されること |
|---|---|---|
| `smoke` | **golden 81 ケース**（`path:line:col:linter:severity:text` を**正規化なしで完全一致**） | check レベルの出力が 1 文字でも動いたら落ちる |
| 〃 | reject ゲート | 「upstream が起動を拒む設定」で guff も拒む |
| 〃 | Go stdlib port differential（time / url / template / regexp） | SA100x のパーサ移植が本物の stdlib と一致 |
| `unit` | — | `cargo test --workspace` 約 3,100 件（「guff が撃つ」ことの回帰網。ground truth ではない） |
| `isolate` | **114 linter を 1 本ずつ** golangci-lint と実行比較 | 複数 linter 構成に隠れるパリティ穴 |
| 〃 | fileset ゲート | 両ツールが**同じ .go ファイル集合**を解析している |
| `oss-pr` | OSS **5 リポ**を各自の `.golangci.yml` で golangci-lint と比較 | 実リポでの precision / recall と worker panic / ill-typed 数 |
| 〃 | `corpus/shapes.py check --offline` | 必須の入力形（generics / cgo / build tags …）を誰かが踏んでいる |

### PR では回らないもの

| 何 | どこで回るか | 埋め方 |
|---|---|---|
| **OSS nightly tier**（consul / grafana / containerd。findings の大半はここ） | main への push | **PR に `nightly-corpus` ラベル**を付ければ PR でも回ります（2026-08-15 追加）。解析系を触る PR は付けること |
| shape ledger の**再測定**（`check` の非 offline） | main への push / ラベル付き PR | 同上 |
| `weekly` tier 3 リポ | **どのジョブにも無い** | 走らないゲートは何も守れない（§Phase 5 と同じ話） |
| **性能・RSS**（`regress/run.sh`） | ローカルのみ | baseline がマシン固有なので CI では判定しない。`docs/PERF_TASKS_V5.md` §7 の手順で手で回す |

### 緑でも残っている既知の差分

- **ratchet**: `revive` / `staticcheck-{sa,st,qf,s}` の golden は**既知の差分件数を凍結**しています。
  増えたら落ちますが、**今ある差分は緑のまま**です。
- **allowlist**: `compat/allowlists/*.txt` に記録した OSS の既知差分（例: consul 3 件）は
  理由と日付つきで除外されています。
- **比較キーの穴**: `compat/normalize.py` は OSS 比較で column / severity / SuggestedFix を見ません（§1）。
  **golden だけがそこまで見ます**。つまり **golden に入っていない check は、列がずれても CI は緑です。**
  linter 単位では 2026-08-24（続き 30）に **116/116 が golden case を持つ**状態になり、
  `compat/tests/test_golden_coverage.py` がそれをゲートにしています
  （新しい linter が golden 無しで入ってきたら落ちる）。
  **check 単位ではまだ穴があります** —— 1 linter に多数 check を持つ
  staticcheck / gocritic / revive / govet / gosec の中身と、
  各 linter の「腕」の網羅は別の話です（続き 30 の「次にやること」1）。

---

## 2. フェーズ

各フェーズは独立に着手・完了できる。番号は推奨実行順（安価で以降の判断材料になるものが先）。

### Phase 0 — カバレッジ台帳 `[完了 2026-08-07]`

**目的**: 「何がテストされていないか」を数字にする。以降の全フェーズの優先度をこれで決める。

- guff が実装する全 check ID をインベントリ化する。
  概算 **550+**（staticcheck 167 + gocritic 104〜106 + revive 100 + gosec 約 40 + govet 29 +
  単一 check linter 約 100）。
- 各 ID について 3 列を埋める: **単体テストで発火 / isolate で発火 / OSS corpus で発火**。
  → **どこでも発火しない check = 完全未検証**。これが Phase 3 のターゲットリストになる。
- golangci-lint 公式 jsonschema から全設定キーを抽出し guff の config と突合し、
  「パースするが実効なし」を機械的に洗い出す（→ Phase 4 のターゲットリスト）。

**成果物**: `compat/coverage.py`、`compat/coverage/{inventory,observed}.json`、`docs/COVERAGE.md`

```bash
./compat/coverage.py inventory   # guff のソースから実装済み check を列挙
./compat/coverage.py observe     # 実行アーティファクトを走査（既存台帳にマージ。--reset で作り直し）
./compat/coverage.py report      # docs/COVERAGE.md を生成
```

`compat/results/` と `regress/results/` は gitignore されているため、台帳は**累積**方式
（どこかのマシンで発火した check は `observe --reset` するまで `fired` のまま）。
`inventory.json` / `observed.json` はコミットする。

**Done when**: ✅ 達成。

#### 結果（2026-08-07）

**guff は 548 check を実装している。そのうち 222（40.5%）は、どのテストでも一度も発火していない。**

| 状態 | 件数 | 割合 | 意味 |
|------|-----:|-----:|------|
| `fired` | 206 | 37.6% | isolate / OSS / regress の実行で発火（＝ golangci-lint と実際に突合された） |
| `unit-only` | 120 | 21.9% | Rust 単体テストが ID に言及するのみ。**golangci-lint との突合はゼロ** |
| `never` | **222** | **40.5%** | **どこでも発火していない = 完全未検証** |

linter 別の内訳（`never` 上位）:

| linter | checks | fired | unit-only | never |
|--------|-------:|------:|----------:|------:|
| staticcheck | 161 | 46 | 1 | **114** |
| gocritic | 107 | 8 | 9 | **90** |
| govet | 30 | 12 | 2 | **16** |
| revive | 100 | 14 | 85 | 1 |
| gosec | 35 | 13 | 22 | 0 |
| 単一 check linter 109 + formatter 6 | 115 | 113 | 1 | 1 (swaggo) |

**読み取れること**

1. **単一 check の linter はほぼ網羅されている**（isolate が 1 件ずつでも撃たせているため）。
   問題は「1 linter = 多数 check」の 5 つに集中している。**staticcheck + gocritic だけで
   `never` の 204/222 = 92%** を占める。→ Phase 3 はこの 2 つから着手する。
2. **gocritic の 90 件未発火は特に危険**。2026-08 の sweep は 104-checker fixture で
   156/156 を達成したが、**その fixture はどのゲートからも実行されていない**。
   一度きりの手作業の結果であり、退行しても誰も気付かない。
   → Phase 3 の最初の一手は「既存の gocritic fixture をゴールデン化してゲートに載せる」。
   新規 fixture を書く必要すらなく、最も安価に 90 件を回収できる。
3. **revive は 85 件が `unit-only`**。単体テストは ID に言及しているが golangci-lint と
   突き合わせたことは一度もない。「撃つこと」は確認済みで「同じものを撃つこと」は未確認。
4. `unit` 列は Rust テストソースの静的スキャン（下限値）。ID に言及していることの証明であって、
   アサーションが意味のある内容である証明ではない。

**残タスク（Phase 0 の未完部分）**

- golangci-lint 公式 jsonschema からの設定キー抽出と guff config との突合は**未着手**。
  Phase 4 の直前にやるのが自然なので、そちらに移す。
- インベントリ件数と `COMPATIBILITY.md` の記載に小さなズレがある
  （staticcheck 167 記載 vs 161 モジュール、gocritic 106 記載 vs 107、govet 29 記載 vs 30、
  revive 100 記載 vs 100）。どちらが正しいか要確認。Phase 3 着手時に潰す。

### Phase 1 — 静かな recall 損失を潰す `[完了 2026-08-07]`

発火しないバグは差分にも出ない。今の仕組みでは**永久に見つからない**類のバグ。

- **ill-typed パッケージのゲート化** `[完了]` — 型検査に落ちたパッケージは analyzer が丸ごと
  スキップされ findings が静かに 0 になる。`compat/health.py` が
  `GUFF_DEBUG_ILL_TYPED=1` の stderr から件数を読み、`compat/baselines/health.json` の
  baseline 超過で fail する（減るのは自由）。**baseline 未登録のターゲットは 0 で厳格**。
- **worker panic をハード fail に** `[完了]` — 同じく `health.py`。panic は baseline を持たず
  **常に fail**。導入時点で helm と kubernetes に `s1032.rs` の
  index-out-of-bounds panic が残っていた（§4 参照）。
- **解析対象ファイル集合の突合** `[完了]` — `compat/filesets.sh`。どちらのツールも
  「解析したファイル一覧」を出力しないので、**絶対にマッチしない `goheader` テンプレート**を
  唯一の linter として両者に食わせる。goheader は 1 ファイル 1 件報告するので、
  出力に現れたファイル集合＝解析したファイル集合になる。

**Done when**: ✅ 上記 3 つが CI ゲート（`compat.yml` の `isolate` / `oss-pr` ジョブ）になり、
現状値が baseline として記録されている。OSS 8 ターゲット + isolate 114 ターゲットすべてで
ファイル集合が完全一致。

#### file-set プローブの盲点（既知の限界）

goheader は「最初のコメントが `//go:` ディレクティブのファイル」を検査しないので、
**その種のファイルはプローブに写らない**（＝ build tag 付きファイルの多く）。
両ツールが同じ規則でスキップするため比較は成立するが、その集合の中での差異は見えない。
強化するなら `go list` の出力を第三の集合として突き合わせる。

### Phase 2 — `linters.default: all` tier の追加 `[ハーネス完成 2026-08-07 / 差分の解消は未着手]`

現行 OSS tier は各リポの実 config を使うため 7 linter しか動いていない。
**同じ 8 リポに全 linter 有効の tier を追加**するだけで、手書き fixture では絶対に出ない
実コードの形が 114 linter 全部にぶつかる。既存ハーネスの引数追加で済む、最もコスパの良い一手。

**ハーネス**: `./compat/run.sh --oss --tier pr --all-linters`（`compat/all_linters.py`）。
リポの `run` / `linters.exclusions` / `linters.settings` は残し、`linters.enable` / `disable` だけを
`default: all` で置き換える。allowlist は専用ツリー `compat/allowlists-all/`（**空**）。
発見用の tier の差分を OSS の allowlist に混ぜると、通常の OSS ゲートの許容範囲が黙って広がるため。

**初回実測（2026-08-07, pr tier）**

| target | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gin | 2671 | 3778 | 1195 | 44.7% | 31.6% |
| caddy | 17149 | 12058 | 8671 | 50.6% | 71.9% |
| helm | 22311 | 16295 | 13774 | 61.7% | 84.5% |

recall 側（golangci にしか無い）の linter 別上位:
wrapcheck 1614 / wsl_v5 1067 / varnamelen 834 / wsl 819 / nlreturn 758 / paralleltest 588 /
exhaustruct 506 / godot 370 / err113 307 / lll 171。
**いずれも今まで 11 行の isolate fixture 1 件でしか比較されていなかった linter**。
§1 の診断がそのまま裏付けられた形。この差分の解消が Phase 2 の本体で、量から見て複数セッション必要。

**初回実行で即出たバグ（godox の worker panic）**

`crates/guff-comment/src/godox.rs:44` が caddy で 2 回 panic していた。
`line[..kw.len()]` が **UTF-8 の文字境界でない位置**で `&str` を切っていたため
（`// If ≠0 then …` は byte 4 が `≠` の内側、`// ⚠️ Template functions…` も同様）。
上流は `bytes.EqualFold(kw, sComment[0:lkw])` と **[]byte** で比較しており境界の概念が無い。
バイト比較に直すのが移植として正しく、同時に panic も消える。

同じ「非 ASCII コメント」系でもう 1 件。メッセージの切り詰め `&trimmed[..40]` も
**バイト**で切っていたが、上流の `fmt.Sprintf("%.40s...", sComment)` は
条件が**バイト長 > 40**、切り詰めが **rune 40 個**という混在で、
65 byte / 25 rune の行は 1 文字も削られないのに `...` だけ付く。golangci-lint 2.12.2 で確認済み。

修正後 caddy を godox 単独で回して **66/66 P=R=100%**（panic 0）。
**panic していた間、そのワーカーの findings は丸ごと落ちていた** = §1 が言う「差分に出ない失敗」。
godox は caddy の実 config では有効化されていないので、`default: all` tier でしか踏めなかった。

**Done when**: 上表の差分が allowlist ではなく guff 側の修正で解消されている。

### Phase 3 — ゴールデン差分の産業化 `[進行中: gocritic 完了 2026-08-07]`（最大の投資・最大の効果）

`compat/golden/` を新設。**linter 単位ではなく check 単位**で fixture を持つ。

- ゴールデンは `compat/golden/regen.sh` が **golangci-lint 2.12.2 を実際に走らせて生成**する。
  人間が期待値を書かない ＝ 思い込みが混入しない。
- 比較キーを厳格化: `path:line:col:linter:severity:text` を**正規化なしの完全一致**で。
  現行 `normalize_message` は OSS tier 専用に残し、golden tier では使わない。
  消していた 7 種の差分は §5 の台帳に降ろして個別に潰す。
- 各 check に**発火例**と**「紛らわしいが発火しない」negative 例**の両方を置く → 偽陽性も捕まる。
- CI では allowlist 禁止。差分はコード修正か、レビュー付きゴールデン再生成のいずれか。

**着手順（Phase 0 の実測に基づく）**

1. ~~**gocritic**~~ — **完了 2026-08-07**。`compat/golden/cases/gocritic/` として
   ゲート化。`never` 90 → 1（残り `whyNoLint` のみ。§6 参照）。バグ 46 件を回収。
2. **staticcheck** — `never` 114 件。最大の塊。check ごとに fixture が必要で最も重い。
3. **govet** — `never` 16 件。
4. **revive** — `unit-only` 85 件。fixture はあるので golangci-lint と突き合わせるだけ。
5. **gosec** — `unit-only` 22 件。同上。
6. 単一 check linter — ほぼ `fired` 済みだが、比較しているのは 1 件だけ（§1）。
   negative 例の追加と column / severity の比較追加が主眼。
   **goheader は完了 2026-08-07**（§4）。`fired` 件数は 1 のまま動かないが、
   比較していたのは「1 ファイルに 1 件出ること」だけで、位置もメッセージ本文も
   見ていなかった。golden 化して初めて 9 種のバグが出た。
   **単一 check linter の `fired` は「検証済み」を意味しない**という実例。

**Done when**: Phase 0 が挙げた全 check に fixture + golden があり、CI 必須ゲートになっている。
進捗は `docs/COVERAGE.md` の `never` / `unit-only` 件数で測る。

### Phase 4 — 設定・除外セマンティクスの互換テスト `[完了 2026-08-12（9 本目）]`

現在ほぼゼロの層。ユーザーが実際に踏むのはここ。すべて finding-set を変える ＝ 互換性そのもの。

- 各 linter の settings キーを 有効/無効/閾値/リスト で 3〜4 パターン
  → **完了 2026-08-12（9 本目）**。errcheck の `verbose`（6 本目）と staticcheck の
  `checks`（8 本目）に、9 本目が **errcheck 5 / govet 5 / gocritic 7 / revive 9 / gosec 8**
  の 34 ケースを足して、finding-set を変える主要キーを閉じた。
  **各グループの baseline はキーを 1 つも書かないケース**にしてある —— 8 本目の教訓
  （既存ケースがキーを明示していると既定は永久に測られない）どおりで、
  9 本目の 9 バグのうち 4 つはその既定側にあった
- `linters.exclusions.{rules,presets,generated,paths}`
  → **完了 2026-08-11（7 本目）**。`cases/exclusions`（baseline）+ `-rules` / `-paths` /
  `-presets` と `cases/generated{,-lax,-strict,-disable}` の 8 ケース。
  v2 の `issues` は `exclude-rules` を**持たない**（すべて `linters.exclusions` に移った）ので
  この行の後半は v1 の話であり、v1 config を読むための
  `exclude.rs::default_exclude_patterns` 側に残っている
- `issues.uniq-by-line` / `max-issues-per-linter` / `max-same-issues` / `severity.rules`
  → **完了 2026-08-12（8 本目）**。`cases/issues-uniq-by-line`、
  `cases/issues-{limits,max-per-linter,max-same,max-both}`、
  `cases/severity-{default,rules,linter}` の 8 ケース
- `//nolint` の全形（同一行・直前行・`//nolint:a,b`・ブロック・説明付き・不正形式）
  → **完了**（`cases/nolint`。nolintlint の settings は `nolint-strict` / `nolint-allow-unused`）
- `run.build-tags` / `run.tests` / `run.go`
  → **完了 2026-08-12（8 本目）**。`cases/run-{tests,tests-off}` /
  `{run-build-tags,run-build-tags-none}` / `{run-go,run-go-122}` の 6 ケース
- `output.path-mode`
  → **このゲートには載せられない**（`run.sh` が `--path-mode abs` を渡し、`golden.py` が
  モジュール相対に正規化する ＝ この設定が変えるものと同じ正規化）。手で実測した結果は
  §4 の 2026-08-12（8 本目）に記録した。**未設定時の既定が食い違っている**（上流は config ファイルの
  ディレクトリ基準、guff は cwd 基準）ことと、**`rel` を上流は config error にする**ことの 2 件

**fixture 1 個 × config N 個の直積**は golden tier がそのまま使える。ケースは
`config.yml` を各自持ち、`sources.txt` が同じ fixture を指すだけなので、
**ハーネスの変更は 7 本目まで 1 行も要らなかった**。設定 1 個の効果は
**2 つのゴールデンの差分そのもの**として読める（`nolint` と `nolint-allow-unused` の差 =
`allow-unused` が消す行）。

8 本目で初めてハーネスに手を入れた。`run.sh` が golangci-lint に
`--max-issues-per-linter=0 --max-same-issues=0` を渡していたためで、
**CLI フラグは config に勝つ＝この 2 キーは測りようがなかった**。フラグを外し、
代わりに**各ケースの config が 2 キーを必ず書くことを `run.sh` が要求する**形にした
（測る対象でないケースは 0）。既定の 50 / 3 で golden が黙って切られる事故も同時に防げる。

### Phase 5 — コーパスの多様化 `[進行中: 台帳・ゲート・サブ形の測定まで完了 2026-08-12（11 本目）/ 踏んでいる形が型検査を通るところまで 2026-08-12（12 本目）]`

現行 8 リポは「普通の Go」に偏っている。踏めていない形:
generics 多用、cgo、build tags、`go.work` マルチモジュール、`vendor/`、`embed`、
テストのみパッケージ、アセンブリ、非 ASCII 識別子、古い go directive、
巨大生成ファイル（protobuf / deepcopy）。

**この列挙自体が推測だったので、まず測った。** `corpus/shapes.py` が各ターゲットの
**実際の package パターン**（＝ゲートが両ツールに食わせる集合そのもの）で `go list` を回す。
チェックアウトの中身とは別物である点が要で、grafana のチェックアウトには `go.mod` が 47 個
あるが `./pkg/...` が解析するのは **1 モジュールだけ**だった。10 本目の初回測定:

| 形 | 8 ターゲット合計 |
|---|---|
| cgo パッケージ / vendor 配下の解析対象 / 非 ASCII 識別子 | **どれも 0** |
| 1 回の実行が跨ぐモジュール数 | **全ターゲットで 1** |
| `go` ディレクティブ | 最古で 1.24 |

`corpus/shapes.py check` が**ゲート**で、必須の形を gated ターゲットが 1 つも踏んでいなければ
落ちる（PR は `--offline` で台帳を信じ、nightly は `go list` で測り直して台帳のズレを捕まえる）。
「踏んでいる」と数えるのは `pr` と `nightly` だけ —— `weekly` はジョブが無く、
**走らないゲートは形を守れない**。踏まないと決めた形は `EXCLUDED` に測定つきで書く（§6 と同じ作法）。

**10 本目で入れたもの**: k9s（`pr`）、cobra（`pr`、`go 1.15`）、
grafana を `./pkg/... ./apps/advisor/...` に広げて **go.work の 2 モジュール跨ぎ**、
非 ASCII は fixture（`compat/golden/cases/nonascii`）。合計 **6 バグ**。

**11 本目で入れたもの**: controller-runtime（generics + codegen。ent は `.golangci.yml` が
v1 でハーネスの前提を満たさない）と、**サブ形の測定**。合計 **8 バグ**。

#### 形の「濃さ」—— covered は「何で」covered なのか `[11 本目]`

`generics` は 7 ターゲットが covered だったが、その中身は測っていなかった。
`corpus/shapes.py` に 3 列足した結果:

| サブ形 | gated ターゲット |
|---|---|
| `genericrecv`（ジェネリック型のメソッド） | consul 21 / grafana 18 —— **pr tier は 0** |
| `genericunion`（`~T` / `A \| B` の型集合） | caddy 4 / consul 8 / grafana 36 |
| `genericalias`（go1.24 のジェネリック型エイリアス） | **全ターゲット 0** |

`genericrecv` は `REQUIRED` に入れた。11 本目のバグのうち 3 つはこの形にしか出ない。
`genericalias` は 12 本目で `EXCLUDED` に移した（測定値 0、`nonascii` と同じく fixture で埋める）。

**そして「踏んでいる」は「解析されている」ですらなかった。** 12 本目が測ったのは
gated ターゲットが持つジェネリックコードが **guff の型検査を通るか**で、
`total += x` も `a < b` も通っていなかった（§4 の 12 本目）。
形の台帳は「入力が来ているか」しか見ない —— **来た入力が捨てられていないか**は、
Phase 1 の ill-typed ゲートと golden の側にしかない。

**ただし「gated ターゲットが踏んでいる」は弱い保証である。** consul と grafana は
`genericrecv` を 39 ファイル持っているのに、revive の受け手の綴りのバグを 1 件も撃たなかった
—— それらの実 config が `exported` を有効にしていないからで、
**ゲートが比較しているのは「形 × 有効な check」の積**のほうである。
形だけの台帳ではそこは守れないので、積は golden 側（`cases/generics`）に置く。

残る候補: tailscale（cgo + tags）、mattermost-server（規模）、
gvisor（unsafe / asm）、kubernetes 全体。

### Phase 6 — 差分ファジングと自動最小化 `[両方とも実装済み 2026-08-12（13 本目）]`

手書き fixture は「思いついた形」しか書けない。

- **縮小器 `compat/reduce.py`** — 差分（または ill-typed）を delta-debugging で最小再現に自動縮小する。
- **ミューテーション生成 `compat/fuzz.py`** — golden fixture を変異させて 2 ツールの一致を問う。
- 両方が使う **`compat/gospans`**（go/ast、stdlib のみ）が「消せる span」と「変えられる site」を出す。

#### 縮小器の要は編集候補ではなく**不変条件**のほう

編集候補が構文単位であること（宣言まるごと / インターフェースの 1 メソッド / 構造体の 1 フィールド /
複合リテラルの 1 要素 / 関数本体を `panic("reduce")` に置換）は必要だが、それは
「行ベースの縮小器が最初の括弧で詰まる」を避けるだけの話である。**本質は
「実 Go ツールチェインが受理し続けること」を絶対の不変条件にした点**にある。

これが無いと ill-typed の最小化は 4 手で破綻する: guff に
`Manager has no field or method GetCache` と言わせる最短経路は
**インターフェースから `GetCache` を消すこと**で、「まだ再現している」としか見ない縮小器は
迷わずそれをやり、壊れたファイルの完璧な再現手順を出力する。不変条件を入れて初めて

```
go build（テストに出る形なら go vet）が受理する  かつ  guff がまだ誤る
```

＝ **guff のバグ**の最小再現になる。§7 が逆方向から辿り着いた
「実 Go ツールチェインに一度も読ませていない fixture は、こうなる」と同じ規則である。

`--build-cmd` が要るのは `go build` が `_test.go` を型検査しないため。
pass 0 で `go list -deps -test` の依存閉包に刈り込むので、
controller-runtime の 359 ファイルは**オラクル 1 回**で 9 に落ちる。

#### ファザーは「findings を保存する」必要が無い

ミューテーションの義務は**コンパイルが通ること 1 つだけ**。比較が
「ミュータントに対する guff 対 golangci-lint」であって「ミュータント対オリジナル」ではないので、
**findings が全部変わる変異も等しく有効なテスト**になる。意味保存の議論が要らないぶん
変異は乱暴に書けて、正しさは Go ツールチェインが無料で保証する。

現在の 6 種は、この codebase で最も戦績の悪い形を狙って選んである:
`paren`（`x` → `(x)`）、`comment`（文の前にコメント行）、`nolint`（行末に `//nolint`）、
`swap`（隣接する 2 文の入れ替え）、`varform`（`x := v` ⇄ `var x = v`）。
**未実装**は識別子リネーム・型の明示/省略・ループ形式変換で、どれも型情報が要る。

ratchet を持つ seed は既定でスキップする（既知差分が全ミュータントに乗って信号が埋もれる）。
`--allow-dirty-seeds` で件数比較に切り替わる。

見つけた形は golden の `cases/parens` に昇格させた（**81 ケース目**）。括弧つきの各関数に**括弧なしの対照**を並べてあるのが要で、「黙るべき側が黙る」と「撃つべき側が撃つ」を同時に固定しないと、**全部黙らせる修正が通ってしまう**。

#### 「型情報が要る」変異は、型検査器を足さずに入った `[2026-08-13（14 本目）]`

識別子リネーム・型の明示/省略・ループ形式変換の 3 つは「どれも型情報が要るので
`gospans` を go/types 込みにするか Rust 側に置くかの判断が先」として保留されていた。
**どちらも要らない。** 理由はこのファイル冒頭の不変条件そのもので、
**変異はコンパイルさえ通ればよく、意味を保つ必要も、したがって正しい必要も無い**。
型が要る編集は「やってみて `go build` に捨てさせる」で足りる。
ここで型検査器を足すと、**不健全さがすでに無料で検出できる編集**のために
ファザーの毎パスに importer を背負わせることになる。

したがって 3 つとも「答えがソースに書いてある部分集合」だけを採る:

- `rename` — ファイル内の出現が全部 1 関数の中に収まるローカルを、
  `len` / `fmt` / `err` / 大文字小文字を反転した綴りへ。
  predeclared / builtinShadow / importShadow / var-naming / unexported-naming が
  実際に鍵にしている名前で、`importShadow` の走査範囲は Phase 5 のバグだった。
- `littype` — `x := 1` ⇄ `var x int = 1`。基本リテラルの型はトークン種そのもの、
  複合リテラルの型はリテラルに書いてある。ST1023 / S1021 / revive `var-declaration` の行。
- `rangeint` — `for i := 0; i < 10; i++` → `for i := range 10`。go1.22 未満のモジュールでは
  ビルドが弾く＝正しい答えが無料で出る。

ついでに `for x := 0; …` のようなヘッダ位置の代入は `varform` / `littype` から除外した。
Go の文法がそこに SimpleStmt しか許さないので**構造上コンパイルできない**ミュータントで、
1 周目の 4.5% がこれだった。

### Phase 7 — 上流ドリフト検知 `[完了 2026-08-13（14 本目）]`

他のティアは全部 golangci-lint を止めて「guff は一致しているか」を訊く。
それは**上流が動くまで**しか効かない。動いた瞬間に golden ゲートが赤くなり、
その差分は**どちら側が動いたのか**を何も語らない —— ゴールデンは
「ある瞬間の golangci-lint の答え」であって、比べるべき別の瞬間がどこにも無いからである。

`compat/drift.py` は guff の側を止めて逆を訊く:

```
keys(golangci@pin, case)  vs  keys(golangci@candidate, case)      ← guff 非依存
keys(guff, case)          vs  keys(golangci@candidate, case)      ← ピンを上げた日のゲート
```

前者は**上流の changelog を読むのではなく測ったもの**、後者は
「ピンを上げた当日に golden ゲートが何と言うか」。**2 つを並べて読むのが要点**で、
「新しい golden 差分 23 件、うち 21 件は上流が考えを変えた分、2 件は我々の分」なら
計画が立つが、「新しい golden 差分 23 件」では立たない。

finding が 1 件も動かずに変わるものが 2 つあるので別に測る:

- **linter インベントリ** —— `help linters --json` を両方の binary で。linter の追加・削除・
  リネーム・deprecated 化・`standard` / `fast` グループの出入りは、
  **全ユーザの `linters.default` の意味**を変える。
- **config の受理** —— 上流が落とした settings キーは、guff がまだ受ける config で
  golangci-lint を**非ゼロ終了**させる。finding 集合の比較では原理的に見えないので、
  candidate が config を蹴ったケースは `config-rejected` として報告する。

両方の binary は `golden.py write` と同じ「2 回一致するまで回す」規則の下で走らせる
（§ 下記「上流は関数ではない」）。1 回走らせた結果でドリフト報告を作ると
**上流のレースを上流のせいにする** —— 正しいが役に立たない。毎週鳴って、毎週違う名前を挙げる。

`compat/drift-ledger.json`（`--update` が書く）は**見たドリフト**を記録する。
`(pin, candidate)` の組に紐づく —— 2.13.0 をレビューしたことは 2.14.0 について何も言わない ——
ので、candidate が変われば全部が未レビューに戻る。抑止するものは他に何も無く、
役目は「同じ変更を毎週報告し続けるのを止める」だけである。

ピン自体は `compat/pins.json` に移した。5 つの workflow ステップに散らばったリテラルで、
これは `drift.py` が捕まえようとしているのと**同じ失敗**の一段下だった:
古い binary に取り残されたジョブは、誰もベースラインだと思っていないものと比較しながら
OK を出し続ける。

---

## 3. 進捗表

| Phase | 内容 | コスト | 状態 | 最終更新 |
|:-----:|------|:------:|------|----------|
| 0 | カバレッジ台帳 | 小 | **完了**（設定キー突合は Phase 4 へ移動） | 2026-08-07 |
| 1 | ill-typed / panic / ファイル集合ゲート | 小 | **完了** — 3 つとも CI ゲート化。残件だった goheader 位置つきマッチャも移植済み | 2026-08-07 |
| 2 | `default: all` tier | 小 | **ハーネス完成** — `--all-linters`。差分の解消（recall 数千件）は未着手 | 2026-08-07 |
| 3 | ゴールデン差分の産業化 | 大 | **進行中** — gocritic / goheader / **govet（16 本目で 34 pass —— 15 本目の `appends` / `waitgroup` / `hostport` に `testinggoroutine` を追加）** / **gosec（35 rule）** は ratchet なしで完了。staticcheck 160 check（ratchet: **missing 7** / extra 9）と **revive 99 rule**（ratchet: **missing 1 / extra 4** — 全部「上流の importer 盲目」1 クラスで、§6 のとおり**追従しないと決めた恒久差分**。`extra` が 3 でなく 4 なのは 2026-08-11（2 本目）に `time-naming` が加わったため）をゲート化。**stdlib 移植は 5 つとも完了**（SA1000 / SA1001 / SA1002 / SA1007 / SA5009）。**文字列定数をバイト列に**（2026-08-10 5 本目）、**gosec の severity / TryResolve / G602 の再スライス**（2026-08-11）。**18 本目で go/ssa の欠落を 2 つ塞いだ**: `emitStore` の `emitConv`（＝インターフェースへのボクシングが起きる唯一の場所。`MakeInterface` / `ChangeInterface` は空構造体で、一度も emit されていなかった）と `logicalBinop`（値文脈の `&&` / `||` の CFG）。前者で staticcheck-sa の ratchet が **extra 7 → 6**、後者で SA5011 の構文側の当て木を削除。同じく 18 本目でインターフェースのメソッドにレシーバが付き、**errcheck-verbose の ratchet 1/1 を削除**（0/0）。**19 本目で `emitCallArgs` と `isValuePreserving`** —— 呼び出し引数が仮引数型へ変換されるようになり、unparam が上流の `typesImplementing` を IR から組めるようになって `compat/allowlists/controller-runtime.txt` の **unparam 2 件が閉じた**。同じく 19 本目で staticcheck-sa の ratchet を **missing 5 → 3 / extra 6 → 2**（SA1023 の位置と件数、SA4020 の文言、SA9004 の列、**SA4015 は反転していたので IR の腕だけに書き直し**） | 2026-08-14 |
| 4 | 設定・除外セマンティクス | 中 | **完了** — golden に **65 ケース**（nolint 3 / errcheck 7 / exclusions 4 / generated 4 / issues 5 / severity 3 / run 6 / staticcheck.checks 4 / **govet 5** / **gocritic 7** / **revive 9** / **gosec 8**）。ランナー側（`//nolint` の 5 規則と nolintlint、除外規則、`generated` の既定、`max-*` の適用順、v2 の `severity.default`、`run.go` の配線）は 6〜8 本目で閉じ、**linter ごとの settings キー**を 9 本目が閉じた: errcheck の枝刈り／括弧／アサーションの位置、govet の `enable` 優先と既定集合、gocritic の `enabled-tags` が**フィルタではなく和集合**であること・`disabled-tags` の適用順・107 チェッカのタグ表・`boolExprSimplify` が `untyped bool` の条件を見ないこと。revive（confidence / severity / enable-*-rules）と gosec（severity / confidence / includes / excludes）は 17 ケースが一発一致。**16 本目で「上流が起動を拒む config」を tier にした**（`compat/reject/`、12 ケース）—— finding 集合の tier では原理的に表現できない側で、7・8・9 本目が列挙した 8 規則 + 上流の同じ関数の隣にあった 2 規則を実装し、**理由の文言まで上流と一致**することを両ツール実行で確認する | 2026-08-13 |
| 5 | コーパス多様化 | 中 | **進行中** — `corpus/shapes.py` が「どの形の入力がどのゲートにも当たっていないか」を測って CI ゲート化。k9s と cobra を `pr` tier に、grafana を go.work の 2 モジュール跨ぎに。非 ASCII は `cases/nonascii`。**6 バグ**（10 本目）: `linters.disable` の優先順、nolintlint が除外フィルタを素通り、gocritic の `skipTestFuncs` と `importShadow` の走査範囲、printf の `parseIndex` 3 か所、godox の位置。11 本目は**サブ形**（`genericrecv` / `genericunion` / `genericalias`）を測って controller-runtime を足し、**8 バグ**: revive の受け手の綴り 3 種、`var-declaration` の刈り込み、gocritic `newDeref` の型、errorlint の allowed **対**、SSA 系 16 analyzer がメソッドを見ていない、ドット import の使用記録がパッケージ単位、SA1019 の位置と末尾スペース、非推奨インターフェースメソッド、govet printf の引数描画。12 本目は**踏んでいる形が型検査を通っているか**を測って `allX`（型集合を見る述語 7 本 × 演算子 11 箇所）・untyped 定数の型パラメータ変換・go1.24 のジェネリック型エイリアスを入れた —— どれも**落ちるとパッケージ丸ごと ill-typed** ＝ 型依存 analyzer が全部黙る側の欠陥。ついでにエイリアス実体の TypeName が package を持たず revive の `unexported-return` が素通りしていた 1 件。`range` / 送受信（`commonUnder` 系）は 15 本目で解消（`#[ignore]` 解除）。**15 本目は ill-typed をもう一段掘って型検査器の欠陥 6 種**: untyped な「値」の代入可能性（`bool(v != 0)`）、埋め込みを辿らないメソッド署名の遅延解決、`IsComparable` のフラグ読み、`commonUnder`、`convertible_to` のメソッド集合準備、逆方向の型推論（go1.21）。kubernetes は 8 → 1 パッケージ、**16 本目の部分的な明示型引数（`sets.KeySet[string](m)`）で 0** | 2026-08-13 |
| 6 | 縮小器 → 差分ファジング | 中 | **完了** — 道具（`compat/reduce.py` / `compat/fuzz.py` / `compat/gospans`）と、それで見つかる分の消化。1 周目 864 ミュータントで 36 件 = 9 バグ、2 周目（seed 1・2 編集/ミュータント・888 ミュータント）で **4 件 = 4 バグ**（errorlint の `(nil)`、gocritic newDeref の描画ノード、SA1006 の paren、nolintlint の unused を**別の**ディレクティブが打ち消す）。**型情報が要るとされた 3 変異**（rename / littype / rangeint）は型検査器を足さずに実装。ファザー自身の穴も 1 つ（`issue_key` 直マップで related-information 行まで数え、staticcheck-sa の baseline を 5→17 に膨らませていた）。`--recheck` を追加。**15 本目で `--allow-dirty-seeds` を初めて回した**: staticcheck-sa 220 ミュータントで 4 件 —— **4 件とも 1 つの構造的欠陥**（パターンマッチャが根でしか括弧を外していなかった）を別々に指していた。revive 側は上流のレースが乗るので `UNSTABLE` が 60 中 7 出るが、確認を通った 1 件が **revive の括弧の向きは staticcheck と逆**（上流は素の型アサーションで括弧を見ない＝黙る、guff は剥がして撃っていた）を出した。**縮小器に「根集合の ddmin」を第 1 パスとして足した**: ill-typed の再現条件がファイルではなく**どのパッケージを root に入れたか**だったので、64 → 3 パッケージまで落として原因に直行した（`--no-reduce-roots` で無効）。結果 controller-runtime の ill-typed は **16 → 0**、そこで**見えるようになった差分が 17 件**（recall は 100% のまま。うち 3 件はその場で修正、17 件を理由つき allowlist に記録（差分は 20 → 17）） | 2026-08-13 |
| 7 | 上流ドリフト検知 | 小 | **完了** — `compat/drift.py` が 81 ゴールデンケースで `golangci@pin` 対 `golangci@candidate`（guff 非依存）と `guff` 対 `candidate`（ピンを上げた日のゲート）を測り、linter インベントリと config 受理も別に見る。`compat/pins.json` にピンを一元化。週次 workflow（`upstream-drift.yml`）。**2.11.4 で検証**: gosec G124 / govet `inline` / revive enable-all の 5 rule / clickhouselint・gomodguard_v2 の追加と gomodguard の deprecated 化 —— 全部 `since: v2.12.0` と一致。今日は pin == 最新なので 0 件で exit 0。**15 本目が `--update` の経路を初めて通した**: ledger の `why` を `--update` が書く placeholder のままにしておくと**週次ジョブが黙る**（＝ §1 が言っている「見ていないから通っているゲート」の一段上）ことが分かり、placeholder を「レビュー済み」と認めないようにした | 2026-08-13 |

**現在の指標**（`docs/COVERAGE.md` / 2026-08-13、16 本目で再生成）: **551** checks 中
`never` **3** / `unit-only` **1** / `fired` **547（99.3%）**
（母数が 550 → 551 に増えたのは 16 本目の govet `testinggoroutine`。即 golden で `fired`）。
`never` の 3 件は **gocritic `whyNoLint` / govet `cgocall` / govet `framepointer`** ——
**3 件とも §6「恒久的に観測できない」側**なので、潰せる `never` は残っていない。
`unit-only` の 1 件は revive `time-naming`。
それ以前に母数が 547 → 550 に増えたのは 15 本目が govet の `appends` / `waitgroup` / `hostport` を
足したから（3 件とも即 golden で `fired`）。
（計画策定時: 548 checks・`never` 222 / `unit-only` 120 / `fired` 206）

それ以前に母数が 548 → 547 に減ったのは、**SA9010 が上流に存在しないチェックだった**ため削除したから（§4 の
2026-08-08 の 2 本目のエントリ）。これで Phase 0 が残していた「staticcheck 161 モジュール」の内訳が確定し、
guff は上流 `honnef.co/go/tools@v0.7.0` の **160 check をちょうど実装している**状態になった。

`unit-only` が 102 → 21 に落ちたのは 2026-08-10 の revive ゴールデン化（83 件）、
21 → 3 に落ちたのは 2026-08-11 の gosec ゴールデン化（18 件）、
3 → 1 に落ちたのは同日の formatter / swaggo の整理による。
**「撃つことは確認済み・同じものを撃つかは未確認」の在庫は尽きた**（残り 1 件は
revive `time-naming`）。次の投資先は `unit-only` ではなく、gosec の G304 のように
**そもそも実装が無い check**（§4 の 7 本目「次にやること 3」）、
ratchet が残っている staticcheck / revive、そして
**「発火したか」ではなく「何を比較しているか」を増やす** 側にある —— その Phase 4 は
9 本目で閉じたので、次は Phase 5（同じ check に**別の形の入力**を通す）。

`SA1011` が 9 件目から抜けたのは 2026-08-10（5 本目）。**`never` の原因が「実装が無い」でも
「fixture が無い」でもなく、`guff-constant` が文字列定数を Rust の `String` で持っていたために
「valid UTF-8 か」という問いが構造上いつでも yes だった**、という形だった。しかも同じ症状が
`crates/guff-staticcheck/tests/checks_test.rs` の唯一の `#[ignore]` の理由文にも書いてあった。
**`never` の隣に `#[ignore]` を並べるだけで繋がる** —— これは 2026-08-11 に
`compat/coverage.py` へ組み込んだ（`docs/COVERAGE.md` の「`#[ignore]` されたテストが
言及する check」節）。理由文だけでは足りず**テスト本体**まで見る必要がある、という
細部も含めて §4 の同エントリに書いてある。

なお 2026-08-09（4 本目）まで govet は 16 件が `never` に見えていたが、そのうち 1 件
（`govet/testpass`）は**台帳側のバグ**だった: inventory は Rust の**モジュール名**を採り、
observe は**メッセージ接頭辞＝analyzer 名**（`tests`）で照合していたため、
この ID は構造的に一度も観測されえなかった。`compat/coverage.py` が
`Analyzer { name: "…" }` を読むように直してある。**台帳自身も検証対象**という実例。

**この指標だけを見ないこと。** 2026-08-08 の SA4006（教科書どおりの形を 1 件も撃てていなかった）と
2026-08-09 の `uniq-by-line` / SA4017 のベンチ除け（どちらも `fired` 済み check の誤検出）は、
**台帳の数字を 1 も動かさない欠陥**だった。`fired` は「golangci-lint と一度でも突合された」であって
「一致している」ではない。一致の指標は golden の ratchet（現在 missing 7 / extra 9）と
OSS / isolate ゲートの側にある。2026-08-09（3 本目）の SA1002 も同じ形で、
**`fired` 済み・isolate 緑のまま `time.Parse("not-a-layout", …)` を撃ち続けていた**
（上流は撃たない）。

**`fired` ですらない罠**もある。2026-08-09（2 本目）の `lostcancel` は
「not used on all paths」の arm が**走査の死角でだけ発火する**状態で、
`fired` 済み・isolate 緑・golden 未搭載だった。つまり
**「撃っている」ことすら「正しい条件で撃っている」の証拠にならない**。
check 単位で golden に載せる（Phase 3）以外にこれを見つける方法は無い。

---

## 4. セッションログ

新しいセッションはここに追記する。形式: `### YYYY-MM-DD — 見出し` / やったこと / 次にやること。

### 2026-08-07 — 計画策定と Phase 0 完了

**やったこと**
- 既存テスト資産（compat / isolate / regress / crates tests）を実測し、§1 の診断を得た。
- 本ドキュメントを作成。
- `compat/coverage.py` を実装（inventory / observe / report の 3 コマンド）。
- 台帳を初回生成: **548 checks 中 222（40.5%）が一度も発火していない**。詳細は §2 Phase 0 の結果。
- 抽出器の初期バグを 3 件修正: revive の内部モジュール `shared_walk.rs` を rule として数えていた、
  gosec のテスト用 ID `G999` を拾っていた、formatter 6 種が台帳から漏れていた。

**次にやること — Phase 3 の最初の一手（最安・最大効果）**

既存の gocritic 104-checker fixture をゴールデン化してゲートに載せる。

- fixture: `crates/guff-style/tests/testdata/gocritic/{bad,extras,ok}.go`
- この fixture は 2026-08 の sweep で golangci-lint 2.12 と 156/156 一致を確認済みだが、
  **どのゲートからも実行されていない**（＝退行しても気付けない）。
- 手順: `compat/golden/` を作り、この fixture を go module 化 → `gocritic.enable-all` の config で
  golangci-lint 2.12.2 を走らせて `.golden` を生成 → guff 出力と**正規化なし完全一致**で比較 →
  CI 必須ゲートに追加。
- 完了後に `./compat/coverage.py observe && ./compat/coverage.py report` を回し、
  gocritic の `never` 90 → 0 を確認して §3 の指標を更新する。

その後 Phase 1（ill-typed / panic ゲート）と Phase 2（`default: all` tier）は安価なので、
Phase 3 の staticcheck 114 件に入る前に片付けるのがよい。

### 2026-08-07 — Phase 3 ハーネス構築と gocritic のゴールデン化

**やったこと**

`compat/golden/` を新設し、gocritic を最初のケースとしてゲートに載せた。
比較キーは §2 の設計どおり `path:line:col:linter:severity:text` の**正規化なし完全一致**、
**allowlist なし**。ゴールデンは `regen.sh` が golangci-lint 2.12.2 を実際に走らせて生成する。

ケースは fixture を**自分では持たない**。`sources.txt` が正典の置き場所を指し、実行のたびに
`.work/<case>/` へ materialize する。したがって Rust 単体テストとゴールデンは同一のバイト列を
食い、ドリフトしようがない（fixture を編集するとゴールデン差分が出る＝意図した信号）。

**gocritic を載せた結果 163 件中 119 件しか一致せず、残り 44 件はすべて実バグだった。**

| 種別 | 件数 | 内容 |
|------|-----:|------|
| column | 42 | 演算子・`=`・引数・`[`・セレクタといった**内側のトークン**を報告していた。go-critic はノード自身の開始位置を報告する。既存ゲートは column を比較しないので**構造的に検出不能**だった（§1） |
| recall | 2 | `preferStringWriter` が `preferFprint` と重なる場合に checker 内で握り潰していた。それは `issues.uniq-by-line` の仕事であり、golden tier では off なので**findings が丸ごと消えていた** |
| precision | 1 | `boolExprSimplify` が、既に報告した式の**入れ子の被演算子**を二重に報告していた。上流は最も外側の式に対して 1 回だけ警告する |

さらに fixture に `unlambda` を 1 行足したところ **4 種目**が出た:
`unlambda` のメッセージが実ソースではなく `func(...) { return f(...) }` というプレースホルダを
描画していた（2026-08 sweep が他 checker で潰した `astfmt` 描画バグの取り残し。
**一度も発火していなかったので誰も気付けなかった**）。

column バグのうちコメント系 checker（`commentedOutCode` / `commentedOutImport` /
`todoCommentWithoutDetail` ほか計 8 個）は単一の共通欠陥だった。コメント検査は
再パース済み AST 上で走るため位置を解析側 `FileSet` へ写す必要があるが、その写像が
**行だけ**を見ていて列を捨てており、全 findings が column 1 に張り付いていた
（`gocritic.rs` の `line_pos` → `remap_pos`）。

**上流の挙動は推測せず、その都度スクラッチモジュールに書いて golangci-lint に食わせて確かめた。**
`boolExprSimplify` の入れ子規則と `docStub` の報告ノード（FuncDecl は `func` キーワード、
TypeSpec は名前）はこの方法で確定させた。これは今後も踏襲すること。

**結果**

- `./compat/golden/run.sh` → gocritic 164/164 完全一致。CI（`compat.yml` の `smoke` ジョブ）に追加済み。
  check モードは guff しか走らせないので安価。
- 台帳: gocritic `never` 90 → **1**、全体 `never` 222 → **133** / `fired` 206 → **304**。
- 既存ゲートに退行なし: `cargo test -p guff-style` 386 件、isolate 114 target、OSS pr-tier いずれも green。

**次にやること**

1. **Phase 1（ill-typed / panic ゲート）と Phase 2（`default: all` tier）** — 安価。
   staticcheck の大物に入る前に片付ける。なお golden の `run.sh` は既に
   guff stderr の `panic` を検出して fail する（Phase 1 の一部を先取り）。
2. **Phase 3 の続き = staticcheck（`never` 114 件）**。gocritic と違い既存 fixture が無いので
   check ごとに書く必要があり、ここからが本番。`compat/golden/cases/staticcheck-*/` を
   check 群ごとに分割するのがよい（SA/S/ST/QF の 4 ケース、あるいは更に細かく）。
   §5 の #3〜#5（staticcheck のコード剥がし・言い回し・末尾ピリオド）は
   golden tier では正規化されないので、ここで自動的に露見する。
3. その後 govet（16）→ revive（`unit-only` 85）→ gosec（`unit-only` 22）。
   revive / gosec は fixture が既にあるので gocritic と同じ「載せるだけ」の安い手。

### 2026-08-07 — Phase 1 完了（ill-typed / panic / ファイル集合）

**やったこと**

3 つとも `compat/` のゲートになった。いずれも「差分に出ない失敗」を対象にしているので、
**導入した瞬間に、既存ゲートが全部 green のまま隠れていたバグが出た**。

| ゲート | 実装 | 導入時に出たもの |
|--------|------|------------------|
| panic | `compat/health.py`（baseline なし・常に fail） | **helm と kubernetes で `s1032.rs:15` の index-out-of-bounds panic**。`is_permissible_sort` が `call.args[0]` を長さ確認なしで参照していた。`sort.Sort()`（引数 0）は ill-typed なコードにしか現れないが、analyzer はそれを見る |
| ill-typed | 同上（`compat/baselines/health.json` 超過で fail） | baseline を記録: gin/caddy/helm 2、consul 14、grafana 30、kubernetes 10。他は 0 で厳格 |
| ファイル集合 | `compat/filesets.sh` + `filesets.py` | **goheader の位置バグ 2 件**（下記） |

**panic の実害**: findings は 1 件も変わらなかった（8 ターゲット全部 P=R=100% のまま）。
つまり §1 が言うとおり「たまたま無害だった」だけ。ただし kubernetes を `./...` で測ると
panic 前 10 → 修正後 44 パッケージが ill-typed として報告されるようになった。
**panic が解析そのものを打ち切っていた**ということで、実害が出るのは時間の問題だった。

**ファイル集合の測り方**: どちらのツールも解析ファイル一覧を出さないので、
**絶対にマッチしない `goheader` テンプレート**を唯一の linter にして両者に食わせた。
goheader は 1 ファイル 1 件報告するので、出力のファイル集合＝解析したファイル集合になる。
guff 側にデバッグ用の出力を足すより、両ツールを同じ土俵で測れるのが利点。

**これで見つかった goheader のバグ**

1. **位置が GOROOT を指していた** — gin の 92 件すべてが
   `/opt/homebrew/.../internal/goarch/goarch.go:1:1` だった。コメントを読むための再パースは
   独自の `FileSet` を持つのに、その位置をそのまま報告していたため、共有位置空間の
   その offset にたまたま居たファイル＝GOROOT のどこかを指していた。
   **gocritic のコメント系で直したのと同じバグ**（あちらは行だけ写して column 1 に張り付く版）。
   共通ヘルパ `guff_analysis::code::remap_reparsed_pos` に括り出して両方から使うようにした。
   （後日補足: goheader は 2026-08-07 のマッチャ移植でこのヘルパを使わなくなった。
   上流は位置を**ファイル自身の行**から組み立てるので remap が要らない。現在の利用者は gocritic のみ。）
2. **`//go:build` で始まるファイルに誤検出していた** — 上流は「`package` より前の**最初の**
   コメントグループ」をヘッダとし、`ast.CommentGroup.Text` がディレクティブを落とすので
   `//go:build` だけのグループは空になり、そのファイルは検査しない。
   guff は「ディレクティブを読み飛ばして次のグループを探す」実装だったため、
   build tag 付きファイル全部が誤検出になっていた。caddy 1 件 / helm 3 件として現れた。
   ついでに guff が独自に飛ばしていた `+build`（旧形式）は上流ではディレクティブ**ではなく**
   ヘッダ本文として扱われる（`ast.IsDirective` は `word:word` を要求する）ので、これも合わせた。

上流の規則はすべてスクラッチモジュールに書いて golangci-lint に食わせて確定させた。推測はしていない。

**結果**: OSS 8 ターゲット + isolate 114 ターゲットすべてでファイル集合が完全一致。
既存ゲートに退行なし（workspace 2939 テスト / isolate 114 / OSS 全 tier / golden いずれも green）。

**次にやること**

1. **goheader の位置つきマッチャ移植**（`docs/COVERAGE.md` ではなく本節の残件）。
   guff はミスマッチを「ヘッダ先頭で `template doesn't match`」と報告するが、上流は
   **食い違った正確な位置**で `Actual: <残り>\nExpected:<残り>` を出す
   （例: `// Copyright 2020 Someone Inc.` に `Copyright 2020 Nobody Inc.` を当てると `1:19`）。
   現在の `match_header` はヘッダ全体を 1 個の正規表現で見るので位置の概念がない。
   テンプレートとヘッダを並べて読む reader への書き換えが要る（prealloc 移植と同規模）。
   **これが済むまで goheader の golden ケースは作れない**。
   → **完了 2026-08-07**（次節）。
2. Phase 2（`default: all` tier）→ Phase 3 の staticcheck。

### 2026-08-07 — goheader の位置つきマッチャ移植と golden 化

**やったこと**

Phase 1 の残件だった goheader のマッチャを、上流 **go-header v0.5.0**
（`go version -m $(which golangci-lint)` で確認した、golangci-lint 2.12.2 が pin している
まさにそのバージョン）と golangci 側ラッパ `pkg/golinters/goheader` の両方から移植した。

これまでの `match_header` は「ヘッダ全体を 1 個の正規表現にして `is_match`」だった。
上流はテンプレートとヘッダを **1 バイトずつ並べて読み進め**、食い違ったバイトで止まる。
したがって出せるメッセージは 1 種類ではなく 6 種類ある:

| 条件 | メッセージ |
|---|---|
| バイト不一致 | `Actual: <ヘッダ行の残り>\nExpected:<テンプレート行の残り>` |
| ヘッダが余る | `Unexpected string: <ヘッダの残り>` |
| テンプレートが余る | `Missed string: <テンプレートの残り>` |
| const 値の不一致 | `Expected:<値>, Actual: <ヘッダ行の残り>` |
| regexp 値の不一致 | `Pattern <re> doesn't match.` |
| ヘッダ無し／空 | `Missed header for check` |

**位置の出どころ**（これが一番の落とし穴）

ラッパは `LineStart(loc.Line + 1) + (loc.Position - offset)` という**生のバイトオフセット**を作る。
`loc` は**ヘッダ内**の座標なのに `LineStart` は**ファイル全体**の行を引く。2 つの座標系が混ざっており、
さらに `loc.Position` にはコメントマーカ分の下駄（`//` なら +4、`/* */` なら +1）が乗ったまま、
ラッパが `//` のときだけ 1 を引き戻す。差し引き **`//` は +4、ブロックは +2** がキャレットのズレとして残る。
結果として **1 行目から始まらないヘッダは自分の行から外れた位置に報告される**
（`offset_header.go`: ヘッダは 3 行目なのに `LineStart(1) + 16` を経由して 3:17）。
上流の挙動なので、そのまま再現した。

この計算は同時に**上流の build ディレクティブ除けでもある**: 位置を持たない issue は
`Location{0,0}` に落ち、`//` ヘッダでは `0 - 1 < 0` になって**捨てられる**。
Phase 1 で「`//go:build` のみのファイルを報告しない」を結果として合わせていたが、
機構はこれだった（guff は `header.is_empty()` で `continue` していた）。
今回どちらの経路も上流と同じ形にした。

**移植中に出た guff 側のバグ**（すべて上流に実際に食わせて確認。推測なし）

| # | 内容 |
|---|------|
| 1 | メッセージと位置が丸ごと違う（`template doesn't match` 1 種のみ・常にヘッダ先頭） |
| 2 | `{{ .YEAR }}` の dot を剥がしていた。上流 v0.5.0 は剥がさないので `.year` は**未定義値**（`Template has unknown value: .year`）。しかも `//` ヘッダではその issue 自体が上記の `< 0` で捨てられ、**ブロックコメントのファイルにだけ出る** |
| 3 | `migrate_old_config`（`{{ YEAR }}` → `{{ .YEAR }}`、`{{ SOME VALUE }}` → `{{ .SOME_VALUE }}`）は v0.5.0 に**存在しない**変換。上流は名前を小文字化・trim するだけで空白も保つ（`{{ SOME VALUE }}` は `some value` を引く）。削除 |
| 4 | 組み込み値名を `YEAR_RANGE` / `year_range` としていた。上流は **`year-range`**（ハイフン）。`YEAR_RANGE` は未定義値 |
| 5 | inline template を `trim()` していた。上流は**逐語**で使う（`template-path` から読んだときだけ TrimSpace）。末尾改行は `Missed string: \n` として出る |
| 6 | `/* * … */` の star block の `*` を剥がしていた。上流は剥がさないので `Actual: * Copyright …` になる |
| 7 | 空のブロックコメント（`/* */`）を skip していた。上流は `Missed header for check` を 1:1 で報告する（`//` の空ヘッダとは違い、こちらは捨てられない） |
| 8 | regexp 値をテンプレート全体の正規表現に埋め込んでいた。上流の `RegexpValue.Read` は**非アンカー**で、残りのどこかにある最初のマッチを探し**その末尾までカーソルを進める**（任意のテキストを読み飛ばせる）。また旧実装は `is_match` だったのでヘッダ先頭の余分なテキストも通していた |
| 9 | `mod-year` / `mod-year-range` が未定義だった。上流は毎回この 2 つを登録する |

上流の rune / byte の非対称（`ConstValue.Read` は値の**rune**を回しつつ `Peek` は**バイト**を返すので、
非 ASCII を含む const 値は決してマッチしない）も含めて再現した。

**恒久的な差分（1 件）**: `mod-year` / `mod-year-range` を guff はファイルの **mtime** から取る。
上流は `git log` のコミット日時を優先し、失敗時のみ mtime に落ちる。ファイルごとに git を
起動するコストが見合わないため。git チェックアウト内では値が食い違いうるので、
**golden fixture ではこの 2 つを使わない**こと。

**golden ケース**

`compat/golden/cases/goheader/` を新設。fixture は Rust 単体テストと同じ
`crates/guff-style/tests/testdata/goheader/` を指す（golden tier の規約どおり case は fixture を
所有しない）。上の 6 メッセージ全部と、ブロックコメント／star block／ディレクティブのみ／
空ブロック／行 1 以外から始まるヘッダ／regexp 値の成否を 15 ファイルで撃ち分ける。

- `./compat/golden/run.sh --case goheader` → **11/11 完全一致**（正規化なし・allowlist なし）。
- 既存ゲートに退行なし: gocritic golden 164/164、`cargo test -p guff-style` 402 件
  （lib 117 + 統合 285）、isolate-goheader P=R=100%、file-set ゲート 114 target 一致。

なお `compat/filesets.sh` の file-set プローブは goheader を使うので、この移植で
メッセージは変わったが**プローブの成立条件（1 ファイル 1 報告）は変わらない**。
`//go:build` のみのファイルが写らないという §Phase 1 の盲点もそのまま（機構が同じなので）。

**次にやること**

Phase 2（`default: all` tier）→ Phase 3 の staticcheck 114 件。

### 2026-08-07 — Phase 2 ハーネスと godox の panic

**やったこと**

`./compat/run.sh --oss --tier pr --all-linters` を追加（§2 Phase 2 に実測値と設計）。
初回実行で **godox の worker panic 2 件**（非 ASCII コメントでの `&str` 境界外スライス）と、
同じ系統の切り詰めバグ（バイト vs rune）が出た。どちらも修正し、
caddy を godox 単独で回して 66/66 P=R=100%。

**次にやること**

1. Phase 2 の差分解消。recall 側の上位 10 linter（wrapcheck / wsl_v5 / varnamelen / wsl /
   nlreturn / paralleltest / exhaustruct / godot / err113 / lll）で 7000 件超を占めるので、
   **linter を 1 つ選んで golden ケース化 → 潰す**を繰り返すのが筋。
   precision 側（guff にしか無い）も caddy / helm では guff の方が多いので、
   偽陽性の調査も要る。
2. Phase 3 の staticcheck（`never` 114 件）。

### 2026-08-08 — staticcheck 161 check のゴールデン化（Phase 3）

**やったこと**

`compat/golden/cases/staticcheck-{sa,s,st,qf}` を新設し、**staticcheck 161 check 全部**を
ゴールデンゲートに載せた。gocritic と同じく fixture は新規に書いていない:
`crates/guff-staticcheck/tests/testdata/<check>/` が既に check ごとの
`bad.go` / `ok.go` を持っていたので、`sources.txt` がそれを指すだけで済んだ。
Rust テストは各ファイルを**単独のパッケージ**として型検査するので、golden 側も
`<check>/<stem>/` と 1 ファイル 1 ディレクトリに materialize している。
config は `staticcheck.checks: [all]`（既定で off の ST 6 件も含む）。

**fixture が実 stdlib では通らなかった（7 ファイル）**

単体テストの stub は `binary.Write(w any, ...)` のように引数を `any` で持っていたため、
`var w any` を渡す fixture が通っていた。実 toolchain は `io.Writer` を要求して落ちる。
sa1003 / sa1014 / sa1020 / s1021 / sa4018 を実際の型に直し、stub にも `io` を足した。
**単体テストの stub が緩いと fixture が現実の Go から乖離する**という一般則の実例。

さらに sa9009 の `ok.go` は `//go:noinline` を `package` の前に置いていた（＝ misplaced
compiler directive）。golangci-lint は**パッケージが 1 つでもコンパイルに失敗すると
他の linter の出力を丸ごと落とす**ので、この 1 ファイルのせいで sa ケースの
ゴールデンが 1 件だけになっていた。ゴールデン生成時は `typecheck` finding の混入を疑うこと。

**初回の突合: 506 件中 333 件しか一致しなかった（差分 173/160）**

| 種別 | 件数 | 内容 |
|------|-----:|------|
| column | 103 | 内側のトークンを報告していた（演算子・`(`・`=`・セレクタ名） |
| メッセージ本文 | 約 25 | プレースホルダを出していた／型名を完全修飾していた／Go の stdlib エラー文言と違う |
| recall / precision | 残り | SA4017 の purity 推論、S1030 の未検出、S1037 / SA9010 の誤検出 など |

**column 103 件のうち 67 件は共通の 1 箇所だった。**
`guff_analysis::pattern_match::match_pos`（「マッチしたノードの診断位置」を返す共有ヘルパ、
**38 の check が使用**）が `BinaryExpr → OpPos` / `CallExpr → Lparen` /
`AssignStmt → TokPos` を返していた。上流 honnef の `report.Report` はノードを受け取って
`node.Pos()` を使う。`guff_ast::commentmap::node_pos`（Go の `ast.Node.Pos()` 相当が既に実装済み）に
委譲するだけで 67 件が一致した。gocritic の `remap_pos` と同じ「共有ヘルパ 1 箇所の欠陥が
数十 check に波及」パターン。

個別に直したもの: SA4000 / SA4003 / SA4008 / S1002 / S1003 / S1004 / S1009（BinaryExpr →
左辺の開始）、SA1006 / SA1013 / S1032（CallExpr → callee の開始）、SA1016（引数式の開始）、
SA4017（IR が call 命令に lparen を刻むので `lparen → CallExpr.Pos()` の写像を作った）、
ST1016（最初のメソッドの**名前**）、ST1019（ImportSpec の開始＝別名があれば別名）、ST1008（名前付きフィールドは**最後の名前**、無名なら型）、
ST1020 / ST1021 / ST1022（**行だけの写像で column 1 に張り付いていた**。
`remap_reparsed_pos` に差し替え。ST1020 / ST1022 は fixture が column 1 の
doc コメントしか持っていなかったので**差分に出ていなかっただけ**）。

**メッセージ本文（上流の挙動はすべてスクラッチモジュールで確認。推測なし）**

| check | 直した内容 |
|---|---|
| QF1011 / ST1023 | 型ではなく**型の式**を描画する。`import t "time"` で `var d t.Duration` は `t.Duration` と出る（実測） |
| QF1004 | メッセージは正典名（`strings.ReplaceAll`）、**suggested fix だけが別名**（`s.ReplaceAll`）。単体テストが逆を assert していた |
| QF1012 | `[]byte(...)` の変換を残す |
| S1004 | `bytes.Equal(a, b)` と実引数を描画。別名 import でも `bytes` と綴る（実測） |
| S1011 | `x = append(x, y...)` と実識別子 |
| S1020 | `when ok is true, i can't be nil` と実識別子 |
| S1001 | `copy(to, from)`（上流は固定文言。実識別子ではない） |
| S1016 | 型名を**現パッケージ相対**で描画（`render::type_string_rel` を追加） |
| ST1018 | エスケープ列の引用符を `'` に |
| SA9002 | 8 進数を Go の `0NNN` 形式に（Rust の `{:#o}` は `0oNNN`） |
| S1003 | `render_expr` が型式（ArrayType / MapType / ChanType / Ellipsis / SliceExpr）を `<expr>` に落としていた。`[]byte("x")` が出せるようにした |

**型検査器の実バグ**（golden の ill-typed ゲートが発見）

`(*T).Foo(nil)` — **ポインタ受信者のメソッド式**を `invalid indirect of T (Type)` で
拒否していた。`Checker::star_expr` が `*x` を常に間接参照として扱い、
オペランドが型のとき `*T` が**ポインタ型**になる分岐（go/types `exprInternal` の
`typexpr` ケース）を持っていなかった。`(*bytes.Buffer).WriteString` のような形は実コードにも出る。

**残差分 70 件と ratchet**

残りは重い 3 クラス:

1. **SA4017 の purity**（missing の大半）— 上流は `analysis/facts/purity` で
   依存パッケージまで含めて純粋性を**推論**する。guff は `pureStdlib` の固定リストしか持たない。
   `time.Parse` / `http.StatusText` / ユーザ定義の `errors.New` などが撃てない。
2. **Go stdlib のエラー文言** — SA1000（`regexp/syntax`）/ SA1001（`text/template`）/
   SA1002（`time` のレイアウト解析）/ SA1007（`net/url`）/ SA5009（printf）。
   guff は Rust の `regex` クレート等のエラーをそのまま出している。移植が要る。
3. 個別の recall / precision — S1030、S1037、SA9010、
   `st1005` の無名レシーバメソッド内で SA4017 が撃てない件。

これらは**このセッションでは終わらない**が、CI を赤のままにも、allowlist で消したくもない。
`cases/<name>/ratchet.json`（`missing` / `extra` の上限）を導入した:
**差分は 1 件残らず今まで通り印字される**。抑止は一切していない。件数が**増えたら fail**、
減ったら「baseline を下げろ」と促すだけ。`compat/baselines/health.json` と同じ ratchet 方式で、
0/0 に到達したらファイルごと削除する（残っていると fail する）。

**結果**

- 台帳: staticcheck `fired` 46 → **157** / `never` 114 → **4**。
  全体 `never` 133 → **23**（govet 16 / staticcheck 4 / gocritic 1 / revive 1 / swaggo 1）、
  `fired` 310 → **421**（76.8%）。
- golden ゲート: gocritic 164/164、goheader 11/11、staticcheck 436/506（ratchet 内。ST ファミリは
  `extra` 0 まで到達し、残るのは SA4017 由来の missing のみ）。
- 既存ゲートに退行なし（workspace テスト、isolate、OSS）。

**次にやること**

1. staticcheck の ratchet を 0 に落とす。順番は SA4017 の purity 推論（missing の最大塊）→
   stdlib エラー文言の移植 → 個別 recall。
2. **govet の `never` 16 件**。これで `never` はほぼ 0 になる。
3. **revive の `unit-only` 83 件**。fixture は既にあるが、`stub/dot` のように
   実 Go では解決できない import path を使っているものがあり、
   golden 化には fixture 側の import path を（Rust 側の `collect_stubs` と整合する形で）
   モジュール解決可能な名前に直す必要がある。
4. `guff-revive/src/rules/{exported,package_comments}.rs` と
   `guff-style/src/lll.rs` にも **行だけの位置写像**が残っている（ST1020 系と同じ潜在バグ）。
   revive を golden 化すれば自動的に露見する。

### 2026-08-08 — SA4017 の purity、二重報告、IR 位置写像（Phase 3 続き）

**やったこと**

前節の ratchet（missing 70 / extra 57）を **missing 49 / extra 36** まで下げた。
着手前に残差分を機械的に分類し直したのが効いた。

**解消した 42 件の内訳**（差分件数 = missing + extra）:

| クラス | 件数 |
|--------|-----:|
| 命令／ノードの**位置写像**（go/ssa の内側トークン vs honnef の `Source().Pos()`） | 30 |
| SA4017 の purity（`pureStdlib` 表の移植 + SrcFuncs のメソッド） | 8 |
| **同一 finding の二重報告**（`uniq-by-line` が隠していた） | 3 |
| 上流に存在しない SA9010 の削除 | 1 |

**残っている 85 件の内訳**:

| クラス | 件数 | 備考 |
|--------|-----:|------|
| Go stdlib のエラー文言（SA1000/1001/1002/1007/5009） | 15 | 次にやること 1 |
| SA4017 の**跨ぎパッケージ** purity 推論 | 11 | §7（構造上の非互換） |
| 残る位置／文言／precision | 59 | 次にやること 2 |

前セッションが `why` に書いた 2 クラス（purity・stdlib 文言）は、実測すると
**残差分の 3 割弱**にすぎなかった。最大のクラスは位置写像で、これは前セッションが
AST 側で直したのと同じ欠陥の **IR 側**だった。

#### 1. SA4017 — purity を独立した fact analyzer として移植

`crates/guff-analysis/src/passes/facts/purity.rs` を新設し、上流
`honnef.co/go/tools@v0.7.0`（`go version -m $(which golangci-lint)` で確認した
2.12.2 の pin）の `analysis/facts/purity` を移植した。SA4017 が持っていた
26 名の固定リストは**両方向に間違っていた**:

- `strconv.Itoa` / `strconv.FormatInt` は上流の `pureStdlib` に**無い**
  （sa1030 の fixture で 2 件の誤検出になっていた）。
- 逆に `time.Now` / `time.Parse` / `time.ParseInLocation` / `time.Unix{,Milli,Micro}` /
  `(*net/http.Request).WithContext` と **`(time.Time)` の 40 メソッド**が抜けていた。
  guff のコメントは method 形式を「SSA callee matching が対応するまで DEFERRED」と
  していたが、`code::type_func_name` は既に `types.Func.FullName()` と同じ
  `(time.Time).Equal` を返すので、単に**表に足すだけ**で撃てた。

さらに上流の**推論**（`check` の再帰）も移植した: stub でない・返り値がある・
全パラメータが basic（basic のみからなる struct を含む）・block がある・
`Select`/`Send`/`Go`/`Panic` を含まない・`Store`/`FieldAddr`/`Load` が
stack addr のみ・`Alloc` が heap でない・呼ぶ先が `len`/`cap` か再帰的に pure、
という条件。honnef の IR は `*ir.Load` を持つが guff-ssa は go/ssa と同じ
`UnOp(MUL)` なので、そこだけ読み替えている。

**この推論が上流と一致するかを golden で証明するために fixture を書き足した。**
`sa4017/bad.go` に「推論で pure になる 4 形」（basic 引数の計算関数、それを呼ぶ関数、
`strings.TrimSpace` を呼ぶ関数、basic だけの struct を受ける関数）、
`sa4017/ok.go` に「pure にならない 5 形」（定数 return だけの stub、返り値なし、
非 basic 引数、副作用のある呼び出し、panic）を置いてゴールデンを再生成した。
golangci-lint は bad の 4 件を撃ち ok の 5 件を撃たず、**guff も完全に一致**した。
推測ではなく上流の実測で裏付けた形。

**跨ぎパッケージの推論は再現できない**（§7 に新設）。上流は依存パッケージにも
analyzer を走らせて fact を伝播するが、guff は root パッケージの関数本体しか
IR 化しない。`net/http.StatusText` / `strings.ReplaceAll` /
ユーザ定義パッケージの `errors.New` が該当し、残 12 件の missing はこれ。

#### 2. `buildir` の SrcFuncs にメソッドが入っていなかった

`st1005/bad.go:23` の `errors.New` だけ撃てない件の正体。`guff-lint/src/cli.rs` が
`buildir_src_methods` を **contextcheck が有効なときだけ true** にしていたため、
既定では `SrcFuncs` が package-level 関数だけになり、`func (T) Read()` の中身を
**src_funcs を回す 20 以上の analyzer 全部が見ていなかった**。
上流の `buildssa`/`buildir` は常にメソッドを含む。

**まず既定を true に戻したが、これは prometheus の regress ゲートを落とす。**
`./regress/run.sh --profile full` で `guff_only` 0 → **6**（`scrape/scrape.go:1709-1711` と
`scrape/scrape_append_v2.go:213-215` の SA5011）。cli.rs のコメントが警告していたとおりだが、
**原因は書かれていなかった**ので調べた:

> SA5011 は `if x == nil` の被演算子を `maybeNil[value]` に入れ、deref 命令の
> オペランドが**その IR 値そのもの**かどうかで報告する。honnef の `ir` は **SSI 形式**で
> **σ ノード**を持つため、`if cached { _ = ce.ref }` の中の `ce` は後段の
> `if ce != nil` の `ce` とは**別の値**になり一致しない。上流のコメントは
> 「sigma を通して情報を伝播しないので分岐内の偽陽性を避けられる」と明言している。
> **guff-ssa は go/ssa 移植なので σ ノードが無い**。したがって同じ値として一致し、撃ってしまう。

つまりこれは「メソッドを見せた副作用」ではなく、**メソッドを見せた瞬間に露出する
SA5011 の既存の precision バグ**（メソッドが解析対象外だったので今まで見えなかっただけ）。
σ ノードの導入は guff-ssa の構造変更なのでこのセッションでは扱えない。

そこで **`BuildIrResult::src_funcs_with_methods()`** を追加した。`prog` は既に
パッケージの全関数（メソッド含む）を持っているので、**SSA を再構築せずリストを
差し替えるだけ**（gosec G602 / wastedassign が private に SSA を作り直しているのとは違う）。
SA4017 だけがこれを使う。共有設定は元に戻したので SA5011 は影響を受けない。
regress ゲートは green、golden は `st1005/bad.go:23` を含めて維持。

**残る債務**: src_funcs を回す他の analyzer は依然メソッドを見ていない。
「見せると SA5011 が誤検出する」がブロッカーなので、**σ ノード相当の手当てが
SA5011 に入るまで解けない**。§7 に記録した。

#### 3. SA9010 は**上流に存在しないチェックだった**

guff の 161 check を上流 v0.7.0 の check 集合と機械的に突合した:

```
$ comm -23 guff_checks.txt upstream_checks.txt
SA9010
$ comm -13 guff_checks.txt upstream_checks.txt      # (空)
```

**guff は上流の 160 check をちょうど実装し、その上に SA9010 を 1 個発明していた。**
honnef の v0.5.1 / v0.6.1 / v0.7.0 いずれにも `SA9010` の文字列は 1 つも無い。
`checks: [all]` で撃つ以上その findings は全件 guff 固有 = 誤検出なので、
モジュールごと削除した。Phase 0 が残していた「161 モジュール vs 167 記載」の
食い違いのうち、モジュール側の 1 件はこれで説明がついた。

#### 4. 同一 finding の二重報告 — `uniq-by-line` が隠していたクラス

golden tier は `issues.uniq-by-line: false` なので、**同じ行に 2 回報告する**バグが
初めて可視化された。3 件あり、いずれも既定の `uniq-by-line: true` では
1 件に潰れるため既存ゲートでは原理的に見えなかった。

| check | 内容 |
|---|---|
| SA4022 / SA4029 | 上流の pattern と**同じ形を探す手書きの `preorder_typed` 走査**が併存し、pattern 側（正しい位置）と手書き側（`op_pos` / `tok_pos`）の 2 回報告していた。上流は pattern だけ。手書き側を削除 |
| SA9009 | `File.Doc` と各 FuncDecl の `Doc` を `File.Comments` に**足して**走査していた。Doc は Comments の一部なので doc コメント内のディレクティブが 2 回出る。上流は `f.Comments` のみ |

同型（pattern + 手書き走査の併存）が他に無いかは HEAD 全体を機械的に走査して確認した
（staticcheck 161 ファイル中この 2 つだけ）。

#### 5. IR 命令の位置写像 — go/ssa と honnef の構造的な差

残差分の最大クラス（38 件）。**honnef の `ir` は全命令に AST ノードを持たせ
`Instruction.Pos()` を `Source().Pos()` と定義している**のに対し、guff-ssa は
go/ssa 準拠で内側のトークン（call なら `(`、binop なら演算子、map 更新なら `[`）を
刻む。したがって IR を報告する check は上流より 1 トークン右に出ていた。

前セッションが AST 側（`match_pos` → `node_pos`）で直したのと**同じ欠陥の IR 側**。
共有ヘルパ `guff_analysis::call_node_starts`（`(` / `[` → ノード開始の写像）を追加し、
さらに `callcheck::emit_report` を直した。**`callcheck` は共有フレームワークなので
1 箇所で SA1021 / SA1032 / SA6000 ほかが一斉に直る**（gocritic の `remap_pos`、
staticcheck の `match_pos` に続く 3 例目の「共有ヘルパ 1 箇所」パターン）。

個別に直したもの: SA1015 / SA1025 / SA4010 / SA5007 / SA9007（call ノード開始）、
SA5000（`m[k]` の `[` → `m`）、SA3001 / SA4018（AssignStmt の開始）、
SA4016 / SA4023（BinaryExpr の開始＝左辺）、SA6001（`:=` ではなく `string(key)` 変換ノード）。

**結果**

- golden: gocritic 164/164、goheader 11/11、
  staticcheck **461/510**（前回 436/506）。ratchet は
  sa 43/47→**25/27**、s 12/8→**11/7**、st 11/0→**10/0**、qf 4/2→**3/2**。
- 台帳は §3 を参照。
- `cargo test --workspace` 2958 件 green。
- **prometheus regress ゲート**（`./regress/run.sh --profile full`）: **PASS**。
  `guff_only` 0 / `golangci_only` 0 / P=R=100%。ただし
  **wall 2.330s → 2.450s（許容 2.480s）、peak RSS 2.73GiB → 2.87GiB** の増が残る。
  purity analyzer が全パッケージで IR を 1 周するぶん。許容内だが**余裕は 0.03s しかない**ので、
  次に何か足すときは必ず `--profile full` を回すこと。

  途中で入れた無駄は取り除いてある: `is_pure_stdlib` は
  パッケージパスで足切りしてから名前を組み立てる（全関数で `String` を作らない）、
  `call_node_starts` の AST 走査は **findings が出たときだけ**行う（SA1015 / SA1025 /
  SA4010 / SA5000 / SA5007）、SA4017 は既存の走査に相乗りする。
  これで初回計測の 3.04s → 2.45s。

**次にやること**

1. **Go stdlib のエラー文言**（残差分の次の塊、15 件）。SA1000 は Go の
   `regexp/syntax` のエラーコード（`missing closing ): \`foo(\``）、
   SA1001 は `text/template`（`template: :1: bad character U+007D '}'`）、
   SA1002 は `time` のレイアウト解析（`cannot parse "" as "4"`、
   かつ `not-a-layout` は**エラーにならない**ので撃ってはいけない）、
   SA1007 は `net/url`（`missing protocol scheme`）、
   SA5009 は printf（`Printf format %s reads arg #1, but call has only 0 args`）。
2. 残る個別の位置／文言／precision。**新たに判明した誤検出**（golden に対応する
   golangci-lint の findings が 1 件も無いもの）: SA4015（`math.Ceil(1)` の
   untyped 定数を「converted integer」と見なす）、SA9004（値を持つ const も
   「最初の const だけ型がある」と見なす）、SA4031 / SA5005 / SA9008 / SA4006。
   いずれも上流に食わせて 0 件であることを確認済み。
3. **govet の `never` 16 件**（前節から未着手）。
4. **revive の `unit-only` 83 件**と、`guff-revive/src/rules/{exported,package_comments}.rs`
   `guff-style/src/lll.rs` の行だけの位置写像（前節から未着手）。
   fixture の `import . "dot"` / `import BadAlias "example.com/badalias"` は、
   `tests/support.rs` の `collect_stubs` が `stub/` 配下の相対パスから import path を
   導出するので、**stub を `stub/example.com/<name>/` に置き直せば**単体テストと
   golden の両方で解決できる（`stub/{fmt,os,context,...}` は stdlib の影なので動かさない。
   golden 側は sources.txt で materialize しなければ本物の stdlib が使われる）。
5. **SA5011 に σ 相当の手当て**（§7）。これが入るまで `buildir` の SrcFuncs に
   既定でメソッドを入れられず、src_funcs を回す 20 以上の analyzer の
   静かな recall 損失が残る。**優先度は高い**（見えない損失なので）。

### 2026-08-08 — SA4006 の再建と、位置／文言の残りを一掃（Phase 3 続き）

**やったこと**

ratchet を **missing 49 / extra 36 → missing 30 / extra 19** に下げた。
着手前に残差分を「位置」「文言」「recall/precision」で分類し、安い順に潰した。

| クラス | 解消した差分数 |
|--------|-----:|
| 報告ノードの取り違え（内側トークン／別ノード） | 14 |
| メッセージ本文（プレースホルダ・過剰修飾・実式の未描画） | 12 |
| SA4006 の recall / precision（下記） | 9 |
| 位置が丸ごと落ちていた（`:0:0`） | 2 |

#### 1. guff-ssa が BinOp / TypeAssert に位置を刻んでいなかった

SA4012 と SA5010 が **`:0:0`**、つまりファイル名すら無い状態で報告していた。
`builder::expr` の `binary_expr` / `type_assert_expr` が `emit`（位置なし）を
使っており、go/ssa が渡す `e.OpPos` / `e.Lparen` を落としていた。
go/ssa 準拠に直したうえで、共有ヘルパ `call_node_starts` に
BinaryExpr（`op_pos` → 左辺の開始）と TypeAssertExpr（`lparen` → 被演算子の開始）の
写像を足した。**gocritic の `remap_pos`、staticcheck の `match_pos`、
`callcheck::emit_report` に続く「共有ヘルパ 1 箇所」パターンの 4 例目。**

`crates/guff-ssa/tests/pos_test.rs` は「binop は位置なしで emit される」と
**旧挙動を固定していた**ので、正しい期待値（`+` の行）に直した。

#### 2. 上流の報告ノード / 文言（すべてスクラッチモジュールで実測。推測なし）

| check | 直した内容 |
|---|---|
| SA1005 | 呼び出しではなく**引数**を報告 |
| SA2000 | `wg.Add` ではなく**呼び出し式全体** `wgs[0].Add(2 + 1)` を描画し、call ノードを報告 |
| SA4005 | レシーバの**型名**を出す（`field T.X`）。ジェネリックは `G[K]` ではなく `G`。位置はセレクタの開始 |
| SA5001 | 解決済みオブジェクトではなく**ソース式**（`fn1()` / `rc.Close()`）を描画 |
| SA5004 | `select` ではなく空の `default` 節を報告 |
| SA5010 | 2 つのインタフェース名は `RelativeTo(pass.Pkg)` で**パッケージ相対**、メソッドのシグネチャは**完全修飾**のまま（実測で非対称を確認） |
| SA5012 | 可変長引数を責めるので**最初の可変長実引数**を報告し、`variadic argument` を前置（`f(a,b,c)` と `f(s...)` の両方で確認） |
| S1010 | スライス式ではなく冗長な**高位式** `len(s)` を報告 |
| S1016 | `{` ではなく複合リテラルの開始 |
| S1019 | `make(T)` ではなく**実型を描画**（`make(chan int)`）。位置は size 引数 |
| S1034 | `switch` ではなくガード `i.(type)` |
| S1035 | `'key'` を引用し所属メソッドを付ける（`of (net/http.Header).Set`）。位置は冗長な引数 |
| S1040 | 被演算子と型を描画（`i already has type interface{}`）。位置は被演算子の開始 |
| QF1007 | RHS ではなく宣言文を報告（fix の編集範囲は RHS のまま） |

#### 3. SA4006 は**共通ケースを丸ごと取りこぼしていた**

`c := a; c = b; _ = c` という教科書どおりの形で **1 件も撃てていなかった**。
原因は FP 抑止ヒューリスティック `IdentIndex` の分類ミス:
go/types は `x = v` の `x` を **`Uses` に入れる**（`Defs` に入るのは `:=` と宣言だけ）。
これを「後で読まれている」と解釈していたため、**あらゆる上書きが抑止されていた**。
上流と一致した golden の SA4006 は 0 件で、7 件が missing だった。

同時に上流の走査対象そのものを合わせた:

- 上流は **`*ast.AssignStmt` しか歩かない**。`n++` は `*ast.IncDecStmt` なので
  `func f(n int) { n++ }` は**報告しない**。guff の IncDecStmt 分岐を削除。
- 判定するのは**右辺の値だけ**（`ValueForExpr(rhs)`）。`n += 1` は定数 `1` に
  なるので撃たない。左辺へフォールバックしていた分岐を削除。
- 報告位置は `=` / `:=` ではなく**代入ノードの開始**。
  `if _, ok := i.(int)` は `ok` の話でも **`_` の位置**に出る。
- `MySlice(y)`（ChangeType）や interface へのボクシング（MakeInterface）は
  **値の貼り替えにすぎないので撃たない**が、`string(b)`（Convert）は撃つ。
  4 形を並べて実測で確定させた。

**抑止を緩めた瞬間に OSS で 4 件の FP が出た**（caddy 2 / helm 2）。すべて同じ形で、
**分岐の片方での代入を、合流後に読んでいる**もの:

```go
loadingRules := clientcmd.NewDefaultClientConfigLoadingRules()
if len(settings.KubeConfig) > 0 {
    loadingRules = &clientcmd.ClientConfigLoadingRules{…}
}
// ここで読む — if を通らない経路では最初の値が生きている
```

位置の前後関係だけでは制御フローが見えない。そこで「後続の代入を上書きと
みなすのは**同じ文リストにあるとき（直線コード）だけ**」に制限した。
さらに prometheus で 1 件出た FP は**ループの後退辺**で、代入より
**ソース上は手前**にある読みが値を使っていた（`tsdb/chunks/chunks.go:190`）。
囲むループ本体のどこかに読みがあれば生きているとみなす規則を足して解消。

**fixture が上流と食い違っていた。** `sa4006/bad.go` の 3 つの `// want` は
**どれも golangci-lint が撃たない形**だった（`n++` / `n += 1` / 定数の上書き）。
上流が実際に撃つ 4 形に置き換え、撃たない形は理由付きで `ok.go` に移した。
単体テストは「bad は空でない」としか見ていなかったので**この食い違いを
何年でも隠せた**。golden 化して初めて出た。

#### 4. 残った差分（30 / 19）

| クラス | 件数 | 備考 |
|--------|-----:|------|
| Go stdlib のエラー文言（SA1000/1001/1002/1007/5009） | 15 | 次にやること 1 |
| SA4017 の跨ぎパッケージ purity | 11 | §7（構造上の非互換） |
| SA5011 の σ ノード | 1 | §7 |
| SA4006 の interface ボクシング | 1 | 下記 |
| 残る位置／文言／precision | 21 | 次にやること 2 |

**新たに判明した構造的な穴**: guff-ssa の `MakeInterface` は
**オペランドを持たない空構造体**（`pub struct MakeInterface {}`）。
そのためボクシングは referrer の辺を作らず、`i = n` の `n` が未使用に見える。
上流に合わせる分岐はコードに置いてあるが**現状は発火しえない**。
SA4006 の FP 1 件がこれで、`sa4006/ok.go` に fixture として残してある。

**結果**

- golden: gocritic 164/164、goheader 11/11、staticcheck **447/496**
  （sa 179/177 の 161 一致、s 79、st 138、qf 107）。
  ratchet は sa 25/27→**16/18**、s 11/7→**3/1**、st 10/0→**10/0**、qf 3/2→**1/0**。
- 台帳は変化なし（`never` 23 / `unit-only` 104 / `fired` 420）。
  SA4006 は元から `fired` だったので、**この種の「撃ってはいるが共通ケースを
  落としている」欠陥は COVERAGE.md の数字には出ない**。golden だけが見つけられる。
- `cargo test --workspace` 2958 件 green。
- isolate 114 target / file-set 3 target いずれも一致。
- OSS pr tier: caddy・helm は P=R=100%。**fixture / local / gin の 3 target は
  このセッション前から赤**（SA4017 の purity FP: `mayErr0` / `rawStrToBytes`）。
  stash して HEAD で測り直し、**本セッションの変更とは無関係**であることを確認済み。
  → 次にやること 3。
- **prometheus regress ゲート**: PASS（`guff_only` 0 / `golangci_only` 0 / P=R=100%）。
  wall **2.330s → 2.460s（許容 2.480s）**、peak RSS 2.93→3.07GiB。
  **余裕は 0.02s しかない。** 静かなマシンでないと計測自体が揺れる
  （負荷がかかった状態では 2.80s まで出た）。次に何か足すときは
  `PERF_GUARD` を通してから測ること。

**次にやること**

1. **Go stdlib のエラー文言**（残差分の最大塊、15 件）。前セッションから未着手。
   SA1000 は `regexp/syntax`、SA1001 は `text/template`、SA1002 は `time` の
   レイアウト解析（`not-a-layout` は**エラーにならない**ので撃ってはいけない
   — guff は今も撃っている）、SA1007 は `net/url`、SA5009 は printf
   （`Printf format %s reads arg #1, but call has only 0 args`）。
2. 残る位置／文言／precision 21 件。誤検出は SA4015 / SA4031 / SA5005 /
   SA9004 / SA9008（上流に食わせて 0 件であることを確認済み）、
   recall は SA1011 2 件 / S1030 / SA6001、位置・文言は SA1019（末尾に**空白**が付く）/
   SA1023 / SA4020 / S1037。
3. **OSS pr tier の SA4017 FP**（fixture / local / gin）。purity 推論が
   `mayErr0` / `rawStrToBytes` を pure と誤判定している。**セッション開始時点で
   既に赤**なので、まずここを緑に戻すのが筋。
4. **govet の `never` 16 件**（2 セッション連続で未着手）。
5. **revive の `unit-only` 83 件**と、`guff-revive/src/rules/{exported,package_comments}.rs`
   `guff-style/src/lll.rs` の行だけの位置写像。
6. **SA5011 の σ 相当の手当て**（§7）。src_funcs の静かな recall 損失を解くのに必要。

### 2026-08-09 — `uniq-by-line` の比較キー、SA4017 のベンチ除け、SA5009 の printf 文法

**やったこと**

前セッションが「SA4017 の purity FP」として残した**赤い OSS ゲート（fixture / local / gin）を
緑に戻した**。ただし原因は purity ではなく、**まったく別の 2 つのバグ**だった。
「差分の原因を推測せずに測る」を守った結果、診断名の方が間違っていたことが分かった形。

#### 1. `issues.uniq-by-line` の比較キーに linter が入っていた（fixture / local）

`exclude.rs` の uniq フィルタは `(file, line, linter)` で数えていた。上流
（`pkg/result/processors/uniq_by_line.go`）は **`(file, line)` だけ**で数える。
1 行から出る issue は run 全体で高々 1 件、という意味だった。

そのため `mayErr0()` のように **errcheck と staticcheck の SA4017 が同じ行に出る**形で
guff だけが 2 件報告していた。fixture 2 件 / local 12 件の「guff にしか無い SA4017」は
全部これで、purity は何も間違っていなかった。

**どちらが残るか**も上流の挙動として確定させた。golangci は
`GetOptimizedLinters` で linter を**名前順にソート**し、`Runner.Run` がその順に
issues を append するので、processors が見る時点で**リストは linter 名でグループ化**されている。
`uniq-by-line` はその**先頭**を残す。スクラッチモジュールで確認:

| 同じ行に出る linter | 残るもの |
|---|---|
| errcheck / staticcheck | errcheck |
| godot / lll | godot |
| govet / staticcheck | govet |
| ineffassign / staticcheck / wastedassign | ineffassign |

guff の診断は analyzer×package のグラフ順に出るので、`apply()` の先頭で
**linter 名による安定ソート**を入れた。`max-same-issues` も同じ順序に依存するので、
uniq の直前ではなくパイプライン先頭に置くのが上流と同じ形になる。
副産物として、guff の出力順（guff は最後に位置ソートをしない）も上流に近づいた。

#### 2. SA4017 に上流のベンチマーク除けが無かった（gin）

`internal/bytesconv/bytesconv_test.go:116` の `rawStrToBytes` は本物の残差分だった。
上流 `sa4017.go` は

```go
if code.IsInTest(pass, fn) {
    for param := range fn.Signature.Params().Variables() {
        if typeutil.IsPointerToTypeWithName(param.Type(), "testing.B") {
            continue fnLoop
        }
    }
}
```

つまり **`_test.go` の中で `*testing.B` を取る関数は丸ごと飛ばす**。`BenchmarkFoo` という
名前で照合しないのは、ベンチが実作業をヘルパに投げることがあるため（上流のコメント）。
純粋関数の返り値を捨てるのは、まさに計測のためにやることなので理に適っている。
`fmt_test` パッケージでの `fmt.Sprintf` という上流唯一のハードコード例外も併せて移植した。

スクラッチで 4 形（`BenchmarkX` / `TestX` / `*testing.B` を取るヘルパ / 取らないヘルパ）を
並べ、上流と**完全一致**することを確認済み。

golden fixture は**足していない**。`sa4017/` に `_test.go` を置くと、Rust 側は
`sa_check_bad_ok!`（`bad.go` / `ok.go` 固定）の外になり `testing` の stub も要る一方、
golden 側は「テストファイルだけのディレクトリ」を作ることになるため。
**この挙動は gin（OSS pr tier の常設ゲート）が押さえている** — `rawStrToBytes` が
まさにこの形なので、退行すれば gin が赤くなる。

#### 3. SA5009 — honnef の `printf` 文法を移植

golden の残差分。guff は `Printf call needs N args but has M args` の**1 種類しか出せず**、
上流は 4 種類を撃ち分ける。上流の `checkImpl` を読んで移植した:

| 条件 | メッセージ |
|---|---|
| 引数が足りない | `Printf format %s reads arg #1, but call has only 0 args` |
| 引数が余る | `Printf call needs 0 args but has 1 args` |
| `%[0]d` | `Printf format %[0]d reads invalid arg 0; indices are 1-based` |
| 文法違反（`%` 単独、`%!`） | `couldn't parse format string` |

`honnef.co/go/tools/printf` の文法は正規表現 1 本（`^%flags widthAndPrecision? index? verb`）で、
Go の regexp も Rust の `regex` も **leftmost-first** なので部分マッチ番号がそのまま通る。
guff の旧実装は `%` の直後で `[n]` を読んでいたが、上流の文法では index は
**flags / width / precision の後・verb の直前**にある。

**実測で分かった上流の癖**: `%%` は `Verb.Value == 0` にパースされ、`if verb.Value != -1`
の分岐に入るので **`hasExplicit = true` が立つ**。これは末尾の「引数が余る」検査を
丸ごと抑止するため、**`fmt.Printf("%v %%", 1, 2)` は上流では何も報告されない**。
guff はここで報告していた。11 形のスクラッチのうち 10 形が完全一致し、
残る 1 形は下記の未移植部分。

**未移植（意図的）**: `checkType`（`Printf format %s has arg #1 of wrong type int`）。
verb と型の対応表・Stringer/error/Formatter 判定・要素への再帰が要る別物で、
今回の文言修正とは独立している。移植前も後も guff はこの診断を出さない。

#### 4. nightly tier が腐っていた — 前セッションの SA4006 が 3 件の誤検出を持ち込んでいた

**pr tier だけを回していると足りない。** 今回はじめて `--tier pr,nightly` を回したところ
consul と grafana が赤で、原因を切り分けるために 3 通り測った:

| 測定対象 | consul | grafana |
|---|---:|---:|
| **HEAD（stash 全部）** | 261 / 255（extra **6**） | 0 / 0 ✅ |
| HEAD + 前セッションの未コミット分 | 263 / 255（extra **8**） | 1 / 0 ❌ |
| 上 + 本セッションの 3 変更 | 263 / 255（extra **8**、同じ） | 1 / 0（同じ） |
| **上 + 下記の SA4006 修正（現在）** | 261 / 255（extra **6**） | 0 / 0 ✅ |

読み取れること 2 つ:

1. **前セッションの SA4006 再建は nightly で 3 件の誤検出を新たに出していた**
   （consul `internal/protohcl/unmarshal_test.go:598,600`、grafana
   `evaluator_test.go:432`）。前セッションは pr tier しか回していないので気付けなかった。
   **未コミットのまま放置すればそのまま入っていた。**
2. 本セッションの変更は consul / grafana の差分を 1 件も動かしていない
   （`uniq-by-line` も SA4017 のベンチ除けも findings を**減らす**方向にしか働かないので、
   これは事前の予測どおり）。

**誤検出の正体**: `IdentIndex` が「上書きされる前に読まれたか」を
**ident の位置の大小**で判定していた。しかし Go は**右辺を先に評価する**ので

```go
decoder := u.bodyDecoder(file.Body)
decoder = decoder.SkipFields("type_url")   // 読んでから上書きする
```

では、上書き先の ident（列 2）が読み（列 12）より**左**にあるだけで、
値は生きている。`defs` に積む位置を ident ではなく**代入文の末尾**に変え、
右辺の読みが必ず手前に来るようにした。`c, extra := c.skip("a"), 2` のように
`:=` の一部が新変数な形（このとき `c` は Def ではなく代入対象）も同じ経路で直る。

上流に 4 形（連鎖上書き・`:=` 連鎖・古典的な上書き・読まない呼び出しでの上書き）を
食わせて**完全一致**を確認し、`sa4006/ok.go` に fixture として追加してゴールデンに載せた。

**残る consul の 6 件は HEAD 由来**（本セッション以前からの既存差分）:
SA5011 1（§7 の σ ノード）/ SA9008 2（golden の ratchet にも載っている precision）/
govet `lostcancel` 2 / unparam 1。**nightly tier は誰のループにも入っていないので、
いつからこうなのか分からない。** `compat/results/RESULTS.md`（コミット済み）は
consul を P=R=100% と表示しているので、少なくともその記録より後に劣化している。
→ 次にやること 3。

#### 開発時の落とし穴（記録）

guff の永続 issue キャッシュの salt は `guff_version()` を使う（上流も
version が空でなければ同じ）。**バージョンを上げずにコードを直すとスクラッチ検証が
古い結果を読む**。`compat/` の各ゲートは毎回 `mktemp -d` した空キャッシュ + `--no-cache`
で走るので影響を受けないが、**手で回すときは `--no-cache` を付けること**。

**結果**

- `./compat/run.sh`: fixture **6→4 件で P=R=100%**、local **120→108 件で P=R=100%**（どちらも赤→緑）。
- `./compat/run.sh --oss --tier pr`: gin / caddy / helm **すべて P=R=100%**（gin が赤→緑）。
  これで **OSS pr tier は 5 target 全部が緑**。
- `--tier pr,nightly`: grafana / containerd も緑。**consul だけ extra 6 で赤**だが、
  これは HEAD 由来の既存差分（上記 §4）。前セッションが持ち込んでいた 3 件は解消済み。
- golden: gocritic 164/164、goheader 11/11。staticcheck は 4 ケース合計で
  **golden 515 件中 486 件一致**（内訳は `sa` 162/177・`st` 138/148・`qf` 107/108・`s` 79/82。
  guff 側の件数は sa 179・st 138・qf 107・s 80）。
  ratchet は sa 16/18 → **15/17**、s / st / qf は据え置き。
  （過去のセッションログの「NNN/MMM」は数え方が揃っていないので、以後は
  ケースごとの `match/golden` を書くこと。）
- 台帳（`docs/COVERAGE.md`）の件数は変化なし（547 / `never` 23 / `unit-only` 104 / `fired` 420）。
  **この 3 件はどれも `fired` 済みの check の欠陥**で、2026-08-08 の SA4006 と同じく
  **`never` / `unit-only` の数字には出ない種類**。
  ついでに、削除済みの `SA9010` が「インベントリ外の check ID」として COVERAGE.md に
  残っていたのを潰した（台帳は累積式なので、モジュールを消しても古い実行アーティファクト由来の
  記録が残る）。`observed.json` から当該キーを落として `report` を再生成した。
  **`observe --reset` はしていない** — 今回回していないターゲットで発火した記録まで捨ててしまうため。
- isolate **114 target すべて一致**（`uniq-by-line` は 1 linter だけを有効にする tier なので、
  今回の変更で挙動が変わりうる場所だったが、影響なし）。
- `cargo test --workspace` **2960 件 green**。
- **regress ゲート（`--profile full`）は正しさ緑・wall 時間赤。**
  `guff_only` 0 / `golangci_only` 0 / P=R=100% だが wall が上限 2.480s を超える。
  本セッションの変更が原因かを A/B で切り分けた（バイナリを 2 本焼いて `GUFF_BIN` で交互に 3 往復。
  環境ドリフトを打ち消すため base→mine→base→… の順）:

  | ラウンド | base（本セッションの 3 変更を stash） | mine |
  |---|---:|---:|
  | 1 | 2.550s | 2.540s |
  | 2 | 2.540s | 2.550s |
  | 3 | 2.530s | 2.570s |
  | 平均 | **2.540s** | **2.553s** |

  差は **+0.013s（0.5%）で、ラウンド 1 では mine の方が速い**（順位が入れ替わる＝ノイズ）。
  **本セッションの変更は性能中立。** そして **base 自身が 2.53〜2.55s で既に上限超え**なので、
  この赤は本セッション以前からのもの。単発測定のばらつきも大きく（同一バイナリで 2.58〜2.99s）、
  **残り余裕 0.02s のこのゲートはこのマシンでは判定不能**。
  → ベースライン 2.330s を測ったマシンとの差か、前セッションの purity analyzer 由来
  （前セッションは 2.460s / 上限 2.480s と記録）。**ベースラインの取り直しか、
  purity の実行コスト削減のどちらかが要る。** → 次にやること 0。
- 単体テストを 2 箇所締めた。`sa5009_flags_invalid_printf` は
  `contains("Printf")` しか見ておらず、**間違った文言を何年でも通せた**ので
  文字列全体を固定した。`exclude.rs` には `uniq-by-line` の
  (file, line) キーを固定するテストを足した。

**次にやること**

0. **regress の wall ゲートを判定可能な状態に戻す**（上記のとおり base で既に赤）。
   ベースライン 2.330s は現在のマシンでは再現しない。まず静かな環境で base を複数回測り、
   ベースラインを取り直すか、purity analyzer の全パッケージ IR 走査を削るか決める。
   **これが赤のままだと以降のセッションが性能退行を検出できない。**
1. **Go stdlib のエラー文言の残り 4 件**。今回 SA5009 を片付けたので残りはこれだけになった。
   4 つとも**共通の構造**を持つ: guff は Go の parser を移植せず **Rust の crate で近似**しており、
   受理する集合もエラー文言も違う。近似の継ぎ足しでは埋まらないので、順に移植するしかない。

   | check | 現状 | 必要な移植 |
   |---|---|---|
   | SA1002 | `go_time_layout_self_parse` という手書きヒューリスティック | Go `time` の `nextStdChunk` + `parse`。上流は `time.Parse(s, s)` を**実際に呼んで `err.Error()` をそのまま出す**だけなので、これが唯一の正解。`"12345"` は `cannot parse "" as "4"`（`getnum` が 2 桁読むため month=12 / day=34 / hour=5 とずれて minute で尽きる）。**`"not-a-layout"` は std chunk を 1 つも含まないので上流はエラーにしない — guff は今も撃っている（FP）** |
   | SA1000 | `regex_syntax` crate + Go 風に「軟化」する前処理 | Go `regexp/syntax` の parser。文言は `error parsing regexp: missing closing ): \`foo(\`` |
   | SA1001 | 独自 | Go `text/template` の lexer/parser。文言は `template: :1: bad character U+007D '}'` |
   | SA1007 | `url` crate + `if s == ":"` のハードコード | Go `net/url` の `parse`。文言は `parse ":": missing protocol scheme` |

   **SA1002 が最優先**。他の 3 つは文言違い（両側とも撃つ）だが、SA1002 だけは
   **撃ってはいけないものを撃っている**＝ユーザーに見える誤検出だから。
2. 残る位置／文言／precision（誤検出は SA4015 / SA4031 / SA5005 / SA9004 / SA9008、
   recall は SA1011 2 件 / S1030 / SA6001、位置・文言は SA1019 / SA1023 / SA4020 / S1037）。
3. **consul の残 6 件**（HEAD 由来。§4 の 4 番目を参照）。内訳は
   SA5011 1 / SA9008 2 / govet `lostcancel` 2 / unparam 1。
   SA5011 と SA9008 は既知（§7 と golden の ratchet）だが、
   **govet `lostcancel` 2 件と unparam 1 件はどこにも記録がない** ので、まずここを読むこと。
   あわせて **nightly tier を毎セッション回す**（pr tier だけでは今回のような
   誤検出を持ち込んだまま気付けない）。`--tier pr,nightly` で 3 分程度。
4. **govet の `never` 16 件**（3 セッション連続で未着手）。gocritic / goheader と同じ
   「既存 fixture を golden に載せるだけ」の安い手のはずで、`never` を 23 → 7 に落とせる。
5. **revive の `unit-only` 83 件**と、`guff-revive/src/rules/{exported,package_comments}.rs`
   `guff-style/src/lll.rs` の行だけの位置写像。
6. **SA5011 の σ 相当の手当て**（§7）。src_funcs の静かな recall 損失を解くのに必要。
   consul の 1 件もこれ。

---

### 2026-08-09（2 本目）— consul の残 6 件を潰し、nightly tier を CI ゲートにした

**やったこと**

前セッションの「次にやること 3」（consul の HEAD 由来 6 件。うち govet `lostcancel` 2 と
unparam 1 は**どこにも記録がなかった**）から着手した。3 件とも guff のバグで、
**うち 2 つは「その check が構造的に壊れている」ことの症状**だった。

#### 1. lostcancel の「not used on all paths」は**発火条件が反転していた**

consul の 2 件（`leader_connect_ca.go:1588` / `server.go:1133`）はどちらも

```go
} else if commonCfg.CSRMaxConcurrent > 0 {
    ctx, cancel := context.WithTimeout(context.Background(), csrLimitWait)
    defer cancel()          // ← 直後に defer している。誤検出
```

という形で、**`else if` の本体**にある。旧実装の「使われているか」判定は
`walk_stmts` / `walk_stmt` で本体を歩いていたが、`walk_stmt` は `BlockStmt` しか
再帰せず **`else if`（`Stmt::IfStmt`）に入らなかった**。一方 def を集める側
（`collect_cancel_from_else`）は `else if` に入る。つまり

- def は見つかる
- その def を含む文が「使用箇所の走査」からは見えない

**そして旧実装は def 文の `cancel` ident 自身を「使用」として数えていた**
（`id.id == cancel.cancel_id`）。したがって
**「def 文が見える」＝必ず used ＝ 決して報告しない**、
**「def 文が見えない」＝ used=false ＝ 報告する**。
つまりこの arm は**走査の死角でだけ発火する純粋な誤検出装置**で、
教科書どおりの本物のリーク（`if b { cancel() }` の後に `return`）は
**1 件も報告できていなかった**。isolate の govet fixture は discarded 形しか
持っていないので、この反転は 3 つのゲートすべてを通り抜けていた。

上流（`golang.org/x/tools@v0.46.0`）は `ctrlflow` の CFG を DFS で辿り、
v を参照するブロックを枝刈りして最初に到達した return ブロックを報告する。
guff に CFG は無いので、**文木の上で同じ探索を書き直した**（`scan_seq` の
`Scan::{Bad,Blocked,Fell}` が「return に到達 / 上流の枝刈りに相当 / 次の文へ」）。
スクラッチ 3 ファイル 25 形を golangci-lint 2.12.2 に食わせて**位置・列・文言まで完全一致**。
実測で分かった上流の挙動（すべて推測ではなく計測）:

| 形 | 上流 |
|---|---|
| 参照が 1 本の分岐にしかない | **報告**（def 文 + その分岐を通らない return の 2 件） |
| 報告位置 | 1 件目は `AssignStmt` / **`ValueSpec`**（`var ctx, cancel = …` は `var` ではなく `ctx` の列）、2 件目は return 文 |
| 文言 | `the <変数名> function is not used on all paths …` — **"cancel" リテラルではなく変数名** |
| `if`/`else` の両方が cancel する | 報告しない（ブロック枝刈りで後続に到達できない） |
| `default` のある switch で全 clause が cancel する | 報告しない |
| `default` の無い switch | **報告**（switch を素通りする経路がある） |
| 条件式の中での参照（`if cancel != nil && b`） | 報告しない（def ブロックの残りに含まれる） |
| `return` を持たない関数 | **報告**。2 件目は**関数の閉じ括弧**（CFG の synthetic return） |
| 末尾が `panic()` | 報告しない（return に到達しない） |
| named result への代入 + 裸の `return` | 報告しない（裸 return は named result の使用） |
| 関数外で宣言された変数への代入 | 報告しない（`funcScope.Contains`） |
| `main` パッケージの `main` | 解析しない |

修正後、guff は**上流が出す 2 件目のメッセージ**
（`this return statement may be reached without using the X var defined on line N`）
も出すようになった。旧実装はこれを一切持っていなかったので、
**1 件も報告できていなかった arm の recall がそのまま増えている**。

初回の修正で consul に**新しい誤検出 3 件**が出た。`for { select { … } }` の中の
def で「継続を辿り切った＝関数末尾の synthetic return に到達」と扱ったせいで、
**条件のない `for` から抜ける経路は無い**のに閉じ括弧を報告していた。
継続の連鎖を無条件ループで打ち切るようにして解消（`child_seqs` の `escapes`）。
**pr tier だけ回していたら 3 件とも見えなかった** — nightly を毎回回す理由がこれ。

#### 2. unparam — interface を満たすメソッドを除外していなかった

consul の `(*mockCAServerDelegate).forwardDC - dc is unused` は、同じパッケージの
`caServerDelegate` interface が `forwardDC(method, dc string, …)` を宣言しているので
**シグネチャを変えられない**。上流 unparam は SSA の `MakeInterface` から
「この具体型のどのメソッドが interface に要求されているか」を集めて除外する。

スクラッチで上流の粒度を確定させた:

| 形 | 上流 |
|---|---|
| interface を宣言し、その interface へ変換もしている | 除外 |
| 同じシグネチャだが**メソッド名が違う** | **報告**（＝シグネチャ文字列だけの一致ではない） |
| 同じシグネチャの**普通の関数** | 報告 |
| 宣言済み func 型と一致 | 報告 |
| interface が同名同シグネチャのメソッドを宣言しているが**変換が存在しない** | **報告** |

guff には変換の記録が無いので、**パッケージ内の interface 型が宣言するメソッドと
名前＋パラメータ／結果型で一致したら除外**する近似を入れた（`collect_interface_methods`）。
上流より広い方向（変換の無い interface でも抑止する）と狭い方向（他パッケージの
interface は見えない）の両方にズレるので、モジュール doc に明記した。
**OSS 8 target / isolate 114 target で recall の減少は 0 件**。

#### 3. SA9008 — 上流のパターンは「**シャドウしている ident 自身**」を assert する形だけ

残る 2 件（`event_endpoint_test.go:115` / `http_test.go:1728`）を読むために
上流実装（`honnef.co/go/tools@v0.7.0/staticcheck/sa9008`）を読んだところ、
パターンが

```
(IfStmt (AssignStmt [obj@(Ident _) ok@(Ident _)] ":=" assert@(TypeAssertExpr obj _)) ok _ elseBranch)
```

で、**`TypeAssertExpr` の被 assert 式が左辺 1 個目と同じ ident**（`pattern` の
再束縛は位置と Object を無視した名前比較）であることを要求している。
guff はこれを見ていなかったので `if v, ok := x.(int); ok { … } else { use v }` を
報告していた（上流は報告しない）。`:=` トークンの確認も抜けていた。両方入れた。

**fixture `sa9008/bad.go` 自身が「上流が報告しない形」だった** — golden の extra 1 件は
これが原因。fixture をシャドウ形に直し、`ok.go` に「名前が違う形」「`=` の形」を追加した。

consul の 2 件は**この修正では消えない**（`if err, ok := err.(HTTPError)` は同名なので
パターンには当たる）。上流が黙る理由は残る IR 検証（`irfn.ValueForExpr` +
`irutil.Flatten(v) != shadoweeIR`）で、guff は移植していない。最小再現を計測で切り分けた:

```go
// 報告される
func v4(xs []int) string {
    for range xs {
        err := mk()
        if err, ok := err.(HTTPError); ok { return "a" } else { return fmt.Sprint(err) }
    }
    return ""
}
// 報告されない ← consul と同じ形
func w1(t *testing.T) {
    for _, v := range rows {
        err := check(v.ip)
        if err != nil {
            if err, ok := err.(HTTPError); ok { t.Log(err.StatusCode) } else { t.Fatalf("%v", err) }
        }
    }
}
```

**ループの中で、さらに `if` でネストした assert だと上流は黙る**（ループを外すと報告する）。
IR 値が assert の結果そのものでなくなる（back edge 越しの Phi が疑わしい）ためと見られる。
→ 次にやること 2。

#### 4. nightly tier を CI ゲートにした（＝次の劣化に日付が付くようにした）

`--tier nightly` は `showcase.yml` の日次 cron にしかなく、**赤くなっても誰も読んでいなかった**。
`compat.yml` に **`oss-nightly` ジョブ**を追加し、**main への push ごとに**
consul / grafana / containerd を回す（PR では回さない: コールドな GHA コーパスで 30 分かかる。
代わりに push 前にローカルで `--tier pr,nightly`）。

> **2026-08-15 追記 — PR から nightly を呼べるようにした。**
> 「push 前にローカルで回す」は**覚えていないと守れない規約**で、しかも
> このセクション自身が「pr tier だけでは 3 件の誤検出に気付けなかった」と書いています。
> そこで `oss-nightly` の条件を
> **「main への push、または PR に `nightly-corpus` ラベルが付いているとき」**にしました。
> 解析系を触る PR はラベルを 1 つ付ければ**マージ前に**答えが出ます。
> 付けなければ従来どおり PR では回りません（毎 PR に 90 分は払わない）。
> あわせて `workflow_dispatch` を足したので、ブランチを指定して手で回すこともできます。

**恒久的に赤いゲートは何も日付を付けられない**ので、残る consul 3 件
（SA5011 1 / SA9008 2）を理由と日付つきで `compat/allowlists/consul.txt` に記録した（§5 参照）。
これで **4 件目が出たら落ちる**。

あわせて `run.sh --name <target>` を追加した（1 target だけを回す。tier を跨いでも指定できる。
fixture / local の暖機は省く）。切り分け中は consul 1 本を 40 秒で回せる。

#### 5. regress の tsdb ゲートも赤だった — 原因は `pattern` の `Object` が広すぎたこと

nightly と同じ話が regress にもあった。`./regress/run.sh`（既定の tsdb プロファイル）は
**`guff_only` 1 で赤**で、`regress/baseline.json` は 0 と記録している。前セッションは
`--profile full` しか回していないので気付いていない。

```
+guff tsdb/wlog/live_reader.go:125:42 S1010: should omit second index in slice, s[a:len(s)] is identical to s[a:]
```

対象コードは `r.rdr.Read(r.buf[r.writeIndex:len(r.buf)])`。上流のパターンは

```
(SliceExpr x@(Object _) low (CallExpr (Builtin "len") [x]) nil)
```

で、`Object` は `pattern/match.go` で **`Ident` に委譲**している（`match(m, Ident(obj), node)`）。
つまり**裸の識別子しか束縛しない** — `r.buf` は当たらない。
guff の `match_object` は `NodeRef::SelectorExpr` も受けて `sel.sel` の Object を束縛していたので、
`r.buf[i:len(r.buf)]` に発火していた。

**上流は束縛と再束縛で非対称**なのが罠だった: すでに束縛済みの `types.Object` と
ノードを比べる経路（`match` の `types.Object` arm）は
**`*ast.Ident` と `*ast.SelectorExpr` の両方を受ける**（後者は `r.Sel` の Object を比較）。
したがって直すのは初回束縛だけで、`match_object_id` はそのままが正しい。
`(Object …)` は多くの check が使う共有部品なので、golden 7 / isolate 114 / OSS 8 の
全ゲートで recall の減少が無いことを確認した。

修正後 **regress tsdb は PASS**（`guff_only` 0 / P=R=100%）。

#### 6. 速度: errcheck の `is_error_type` が呼び出しごとに arena を走査＋複製していた

guff の強みは速さなので、`samply` で prometheus `./tsdb/...` を実測した
（`--profile profiling`。`release` は `strip = true` で記号が無い）。
guff プロセスの self サンプル上位は平坦（最大 4.6%）で、単一のホットスポットは無い。
そのトップが **`guff_errcheck::is_error_type` 4.6%** だった。中身は 1 呼び出しごとに:

1. `universe_error()` — **object arena の全走査**で組み込み `error` を探す
2. `artifacts.types.clone()` — `api_implements` が `&mut TypeArena` を要るための複製

これを未チェック呼び出しの**結果型ごとに**やっていた。`Visitor` に
(a) `error` の TypeId、(b) run 全体で 1 つの scratch arena、(c) `TypeId → bool` のメモ
を持たせた（`lockpath.rs` が既に使っている scratch パターンと同じ）。

2 番目に重かった `position::File::position_internal` 4.0% は、呼び元がほぼ
**printer**（gofmt / gofumpt フォーマッタ）だった。printer は 1 ノードごとに行番号を聞くのに
`position_for()` が返す `Position` は**ファイル名の String を毎回複製**していて、誰も読まない。
`File::line_for` / `FileSet::line_for` を足して printer / parser / import から使うようにした。

結果型は繰り返し出てくる（`error`、`int`、`[]byte`、そのパッケージ自身の型）ので、
`TypeId → bool` のメモが**ほぼ全部の問い合わせを吸収する**。
メモは `type_with_name`（型全体を `String` に描画して `"error"` と比較する）より
**手前**に置くこと — これも呼び出しごとに払っていた。

**A/B 実測**（同一マシン、`perf-guard.sh` PASS、バイナリ 2 本を交互に。
`prometheus/.golangci.yml`、`--no-cache`、warm GOCACHE。
`GUFF_DEBUG_CACHE=2` で phase 内訳も同時に取得）:

| 対象 | base | mine | 差 |
|---|---:|---:|---:|
| `./tsdb/...` wall | 0.57 / 0.57 / 0.57 / 0.58 s | 0.53 / 0.53 / 0.53 / 0.52 s | **−0.045s（−8%）** |
| `./tsdb/...` `analyze` phase | 0.20 / 0.19 / 0.20 / 0.19 s | 0.15 / 0.15 / 0.15 / 0.15 s | **−0.045s（−24%）** |
| `./...` wall | 1.81 / 1.89 s | 1.77 / 1.87 s | −0.03s |
| `./...` `analyze` phase | 0.88 / 0.90 s | 0.85 / 0.86 s | −0.035s |

**wall の減り分はそのまま `analyze` phase の減り分**（tsdb でどちらも −0.045s）。
`load_graph` / `typecheck_roots` は動いていない（0.19s / 0.26s のまま）。
**findings は両ワークロードで完全一致**（tsdb は S1010 の誤検出 1 件が消える分だけ 5→4、
それ以外は bit 単位で同じ。full は 20/20 完全一致）。
再プロファイルすると guff の self サンプルは 2454 → 2018（**−18%**）で、
`is_error_type` はトップから消えた。行番号側は割り当てが減っただけで、
単独では測れる差にならなかった（正直に言えば −0.045s はほぼ errcheck の分）。
`./...` で wall がほとんど動かないのは、そちらでは guff の外側が支配的だから
（tsdb のプロファイルでも**サンプルの 24% は `go` プロセス** = `go list` と export data 生成）。
**`--profile full` の wall 赤（2.610s > 上限 2.480s、正しさは 20/20 緑）はそこにある。**

**副産物: `docs/PERF_TASKS_V2.md` §1.3-post2 の「地図」が古い。** あの表は `analyze` を
**0.37s** と書いているが、同じコマンドの実測は **0.85〜0.90s**（`./...`、cold）。
2026-07-30 以降に増えた check の分だけ育っており、
**「analyze はもう小さい / C-4 の期待値も消滅」は現在は成り立たない**。
あの節に日付つきで追記した。→ 次にやること 0。

#### 7. golden に govet-lostcancel ケースを追加

`compat/golden/cases/govet-lostcancel` は上の 25 形の fixture
（`crates/guff-govet/tests/testdata/lostcancel/paths.go`）を指し、**25/25 完全一致**。
これは「次にやること 4（govet の never 16 件）」の最初の一手でもある
（`lostcancel` は govet で唯一 CFG に依存する analyzer なので、
既存 fixture を載せるだけでは足りず、本体を書き直す必要があった）。
Rust 単体テストも `paths.go` を使う（golden と同じバイト列）。
context stub に `CancelFunc` / `WithTimeout` / `WithDeadline`、time stub を追加した。

**結果**

- `./compat/run.sh --oss --name consul`: **guff=258 golangci=255 → allowlist 3 件で緑**
  （修正前は extra 6）。
- `--tier pr,nightly`: gin / caddy / helm / grafana / containerd **すべて P=R=100%**、
  consul は allowlist 3 件のみ。**OSS 8 target すべて緑**。
- isolate **114 target すべて一致**。fixture / local も P=R=100%。
- golden: **7 ケース**（gocritic 164/164、goheader 11/11、**govet-lostcancel 25/25**、
  staticcheck-{sa,s,st,qf}）。staticcheck-sa の ratchet は
  **extra 17 → 16**（SA9008 の 1 件が消えた。missing 15 は据え置き）。
  `sa9008/bad.go` を書き直したので golden を再生成した（新しい 2 件はどちらも一致）。
- weekly tier（vault / kubernetes）も回した: **vault 161/161・kubernetes 5/5 で P=R=100%**、
  panic 0。`compat/results/RESULTS.md` は 3 tier 全部（**OSS 8 + fixture + local = 10 target**）の
  スナップショットに戻してある（直前のコミットは weekly の 2 行を含みつつ consul を
  100% と表示していた）。
- **regress tsdb: FAIL → PASS**（`guff_only` 1 → 0）。
  `--profile full` は正しさ 20/20 緑・wall 2.610s で**赤のまま**（上限 2.480s）。
- `cargo test --workspace` green。
- 速度: prometheus `./tsdb/...` で **0.57s → 0.53s（−0.045s / −8%）**。findings は
  S1010 の誤検出 1 件が消える分以外は完全一致。
- 台帳（`docs/COVERAGE.md`）の件数は変化なし（547 / `never` 23 / `unit-only` 104 / `fired` 420）。
  **今回の 5 件の欠陥はどれも `fired` 済み check のもの**で、台帳の数字を 1 も動かさない。
  ついでに、削除済み `SA9010` が古い実行アーティファクト経由で「インベントリ外の check ID」
  として復活していたので、また落とした（`observe` は累積式なので、
  ローカルに古い `compat/results/` が残っているマシンでは毎回復活する）。

**次にやること**

0. **regress `--profile full` の wall ゲート**（前セッションの 0 番がそのまま残っている）。
   本セッションで analyzer 側は速くなったが**この数字は動かない**（§4 の 6 番: `./...` は
   `go list` / export data 生成が支配的）。**まず `go` 側と guff 側の内訳を測ること** —
   そこを見ずにベースラインを取り直すのも analyzer を最適化するのも当て推量になる。
   tsdb プロファイルでは `go` プロセスがサンプルの 24% を占めていた。
1. **Go stdlib のエラー文言 4 件**（SA1002 / SA1000 / SA1001 / SA1007）。
   前セッションの 1 番のまま。**SA1002 が最優先**（撃ってはいけないものを撃っている）。
2. **SA9008 の IR 検証**（上の最小再現 `w1` vs `v4`）。consul の残 2 件と
   staticcheck-sa golden の extra 1 件が同じ原因。
   `ValueForExpr` 相当が無いので、まず「ループ内 + ネストした if」で上流が黙る
   本当の条件を IR ダンプで確定させること。**推測で近似すると recall を失う**。
3. **SA5011 の σ 相当**（§7）。consul の残 1 件。
4. **govet の `never` 16 件**（`lostcancel` は元から `fired` だったので**減っていない** —
   golden に載せたことと台帳の数字は別の話）。golden ケースの作り方は
   `govet-lostcancel` を雛形にできる。
5. **revive の `unit-only` 83 件**と位置写像（前セッションの 5 番のまま）。

---

### 2026-08-09（3 本目）— SA1002 / SA1007 を「近似」から Go stdlib の移植に置き換えた

前セッションの「次にやること 1」（Go stdlib のエラー文言 4 件）のうち **SA1002 と SA1007 を完了**。
SA1000 / SA1001 は未着手（見積もりは下の「次にやること」）。
staticcheck-sa の ratchet は **missing 15 / extra 16 → missing 13 / extra 13**。

#### 0. 方法 — stdlib オラクルを常設した（`compat/oracles/`）

SA1000 / SA1001 / SA1002 / SA1007 / SA5009 の上流実装は、**定数を stdlib に渡して
`err.Error()` をそのまま出すだけ**である。したがって「チェックを移植する」とは
「**パーサを移植する**」ことに等しい。Rust の crate で近似すると必ず 2 か所ずれる:

1. **受理する集合が違う** → 上流が黙るものを撃つ（FP）／撃つものを黙る（FN）
2. **文言が違う** → 判定が一致していても golden は落ちる

そして「移植した」が「近似より正しい」と言えるのは、**それを検証したときだけ**。
そのために `compat/oracles/` を作った: Go プログラムが**本物の stdlib**を決定論的な
コーパスに掛けて `<入力>\t<hex>\t<結果>` を吐き、Rust 側は同じコーパスを自分の移植に流して
**全行一致**を要求する。期待値は 1 つも手書きしない（`compat/golden/` と同じ規則）。
使い方と Go バージョンの結び付き（下記 3 番）は [`../compat/oracles/README.md`](../compat/oracles/README.md)。

| オラクル | 出力 | 検証対象 | 行数 |
|---|---|---|---:|
| `gotime` | `tests/testdata/gostd/time_parse.tsv` | `gostd::time`（SA1002） | 10,028 |
| `gourl` | `tests/testdata/gostd/url_parse.tsv` | `gostd::url` / `gostd::netip`（SA1007） | 6,441 |
| `goquote` | `tests/testdata/gostd/quote.tsv` | `gostd::strconv` | 739 |
| `goquote-table` | `src/gostd/isprint_table.rs`（生成コード） | `gostd::strconv::is_print` | 720 |

#### 1. SA1002 — `go_time_layout_self_parse` を捨て、`time.Parse` を移植した

旧実装は「既知トークンを `contains` で探す」ヒューリスティックで、**文言も出していなかった**
（`parsing time "X" as "X"` で止まり、`: cannot parse "" as "4"` が丸ごと欠けていた）。
`crates/guff-staticcheck/src/gostd/time.rs` に `nextStdChunk` / `skip` / `getnum` / `getnum3` /
`lookup` / `parseNanoseconds` / `parseTimeZone` / `quote` と `parse` のエラー経路を移植。
`Date()` 以降（ゾーン検索・時刻の構築）は SA1002 が見ないので落とした。

**この 1 件が FP の実体だった**: `time.Parse("not-a-layout", …)` は std 要素を 1 つも含まない
＝自分自身を literal として食い尽くすので、**上流は成功する**。guff は撃っていた。
`"hello"` や `"yyyy-mm-dd"` も同じクラスで、旧ヒューリスティックはこれを全部撃っていた。

fixture も差し替えた。旧 `bad.go` は `"12345"` と `"not-a-layout"` の 2 件で、後者は
そもそも上流が撃たないものだった。いまは `time.Parse` が返しうる **2 つのエラー形**
（フィールドが入力を使い果たす／範囲外）を `"12345"` / `"1234"` / `"123456"` で押さえ、
`ok.go` に「literal だから通る」ケースを移した。

#### 2. SA1007 — `url` crate を捨て、`net/url.Parse` を移植した

旧実装は `url::Url::parse` ＋ `if s == ":"` のハードコードだった。`url` crate は
**WHATWG URL 仕様**であって Go の読む RFC 3986 ではないので、`foobar` と `mailto:a@b.c`
（Go は両方受理）を弾く。旧コードの `if !s.contains(':') && !s.starts_with('/')` は
その一部を場当たりに避けていただけで、網羅されていなかった。

移植したもの: `gostd/url.rs`（`Parse` / `parse` / `getScheme` / `parseAuthority` /
`parseHost` / `unescape` / `shouldEscape` / `validOptionalPort` / `validUserinfo`）、
`gostd/netip.rs`（`ParseAddr` / `parseIPv4Fields` / `parseIPv6` — `parseHost` が
IP-literal に対して呼び、そのエラー文をそのまま包むため）、`gostd/strconv.rs`
（`Quote` / `IsPrint` — `net/url` のエラーは全部 `%q` を通る）。
`url` crate は依存から外した。

`shouldEscape` は Go 1.26 では生成テーブルだが、`gen_encoding_table.go` に
**リファレンス実装がそのまま残っている**のでそちらを移植した（テーブルの再生成は不要）。

fixture は 9 つのエラークラス（missing protocol scheme / first path segment cannot contain
colon / invalid port / invalid URL escape / invalid character in host / missing ']' /
invalid IP-literal / ParseAddr の各種 / invalid userinfo）を 1 件ずつ持つ形に書き直し、
**9/9 完全一致**。`checks_test.rs` の `contains("is not a valid URL")` は
**staticcheck 側のラッパーしか見ておらず、中身の `net/url` エラー（＝ crate 由来で
何とも一致していなかった部分）を素通しにしていた**ので、文字列全体を固定した。

#### 3. `IsPrint` は Unicode バージョンに固定されている（crate では代替できない）

`strconv.Quote` の `\u` 判定は `unicode.IsPrint`。これを `unicode-general-category` crate の
カテゴリ（L/M/N/P/S ＋ ASCII space）で再現しようとすると、**Go 1.26 と crate 1.x の間で
5,812 コードポイントが食い違う** — Go は自分のテーブルが固定された Unicode バージョンで
答えるので、それ以降に割り当てられた文字を crate は printable と言い、Go は言わない。
Go の `strconv` 自身が生成テーブルを持っているのはこの理由なので、guff も
**Go のテーブルのコピー**を持つことにした（`goquote-table` が生成、720 レンジ）。
検証は `quote.tsv` 側で、**全 rune について** `is_print` を Go の答えと突き合わせている。

#### 4. `urlstrictcolons` — 正しさが golangci-lint の go.mod に依存している

Go 1.26 は http/https のホストで「ポート区切りは**最初**のコロン」に変えた
（go.dev/issue/75223）。従来は**最後**のコロンで、`http://h1:5432,h2:5433/db` が通る。
切り替えは `urlstrictcolons` godebug で、**その既定値はメインモジュールの go directive
から決まる**。つまり `url.Parse` の挙動は golangci-lint 自身の go.mod 次第で、
**v2.12.2 は `go 1.25.0`** ＝ 従来（最後のコロン）。実測で確認した:

| oracle の go directive | `http://h1:5432:5433/` |
|---|---|
| 1.24 / 1.25.0 | 通る |
| 1.26 | `invalid port ":5432:5433" after host` |

`compat/oracles/gourl/go.mod` を `go 1.25.0` に固定し、理由をコメントに書いた。
**golangci-lint が go directive を上げたら、ここも上げて golden が動くのを見ること。**

**結果**

- golden: **7 ケース全部**が gate 通過。staticcheck-sa の ratchet は
  **missing 15 / extra 16 → missing 13 / extra 13**（SA1002 で 1/2、SA1007 で 1/1 減）。
  fixture を増やしたので golden の総数は 179 → 194 に増えている。
- isolate **114 target すべて一致**。
- OSS `--tier pr,nightly`: 6 target すべて据え置き（下の「結果」参照）。
- `cargo test --workspace` green。新規テスト: `tests/gostd_time.rs`（10,028 行）、
  `tests/gostd_url.rs`（6,441 URL ＋ 全 rune の `is_print` ＋ quote 29 ケース）。
  この 2 本は `.github/workflows/compat.yml` の `golden` ジョブに載せた。
  **ついでに見つかった穴**: CI は `cargo build` しかしておらず、**`cargo test` を
  どのジョブも回していない**（`config-corpus.yml` の 1 テストだけが例外）。
  つまり Rust 側の 2,800 テストは**ローカルでしか守られていない**。
  今回は新しい差分テスト 2 本だけを速いジョブに載せて済ませた
  （`cargo test --workspace` はコンパイルだけで数分かかるため）。
  **全体をどう CI に載せるかは未決 — 次にやること 6。**
- 台帳（`docs/COVERAGE.md`）の件数は変化なし。SA1002 / SA1007 はどちらも元から `fired`
  だった — **`fired` は「一度でも突合された」であって「一致している」ではない**という
  §3 の注意書きの、また別の実例。

**次にやること**

0. **regress `--profile full` の wall ゲート**（3 セッション連続で残っている）。
1. **SA1000（`regexp/syntax`）と SA1001（`text/template`）**。残る stdlib 移植はこの 2 つ。
   どちらも SA1002 / SA1007 より**一桁大きい**ので、腰を据えて取ること:
   - SA1000 は `regexp/syntax/parse.go` ≒ 2,000 行（文字クラス、Unicode script/property、
     perl クラス、repeat count、flags）。文言は `error parsing regexp: <ErrorCode>: \`<Expr>\``
     で、**`Expr` が「どの部分文字列を指すか」まで一致させる必要がある**。golden 3/3。
   - SA1001 は `text/template` の lexer ＋ parser ≒ 1,400 行。ただし**上流は
     `strings.Contains(err, "unexpected") || strings.Contains(err, "bad character")` で
     絞っている**ので、その 2 クラスを出す経路だけで足りる可能性がある。まず
     `text/template` のどのエラーがこの 2 語を含むか列挙してから見積もること。golden 1/1。
   - 進め方は本セッションと同じで良い: `compat/oracles/` に `goregexp` / `gotemplate` を足し、
     コーパスを決めて tsv を吐かせ、**移植前に**受理集合の差分を測る。
2. **SA9008 の IR 検証**（前セッションの 2 番のまま）。consul の残 2 件と
   staticcheck-sa golden の extra 1 件が同じ原因。
3. **SA5011 の σ 相当**（§7）。consul の残 1 件。
4. **govet の `never` 16 件**（4 セッション連続で未着手）。`govet-lostcancel` が雛形。
5. **revive の `unit-only` 83 件**と位置写像。
6. **`cargo test --workspace` を CI に載せる**（上の「結果」参照）。
   別ジョブにして `Swatinem/rust-cache` を効かせるのが素直だが、
   **まず GHA での実測時間を測ってから**決めること。
   これが無い限り、Rust 側のテストを何本足しても「ローカルでだけ緑」のままになる。

### 2026-08-09（4 本目）— govet 28 pass をゴールデン化（`never` 23 → 9）

**やったこと**

前セッションの「次にやること 4」（4 セッション連続で持ち越されていた govet の `never` 16 件）を消化した。
`compat/golden/cases/govet/` を新設し、既存の `govet-lostcancel` ケースを**そこに畳み込んだ**
（`lostcancel/paths.go` の 27 件は 1 行も変わっていないことを diff で確認済み）。
gocritic と同じく fixture は新規に書いていない — `crates/guff-govet/tests/testdata/<pass>/` が
既に pass ごとの `bad.go` / `ok.go` を持っていたので、`sources.txt` がそれを指すだけで済んだ。

**ゲートに載せた瞬間に 17 件の差分が出て、全部が実バグだった**（fixture を足して更に 3 件）。

| 種別 | 件数 | 内容 |
|------|-----:|------|
| 報告位置 | 11 | 内側のトークン（`(` / `{` / 演算子）を報告していた |
| メッセージ本文 | 2 | `bools` が Token の Debug 名、`slog` が callee 名を落としていた |
| recall | 3+3 | `buildtag` / `directive` が package 節より後のコメントを**原理的に見られなかった** |
| precision | 1 | `sigchanyzer` の条件が**反転**していた |
| 文字列デコード | 1 | 共有ヘルパ `unquote_go_string` が `\xHH` / 8 進 / `\a` などを**壊して**いた（下記 5） |

#### 1. `bools` — `split` が上流と逆順だった

上流 `split` は `a || b || c` を **`[c, b, a]`** で返す（doc comment に明記されている）。
`checkRedundant` はその順に走るので、重複の報告は**左側**に落ちる。guff は順方向に
畳んでいたため右側に落ちていた。`checkSuspect` も同じ順序に依存していて、
`suspect or: a != 1 || a != 2` の**引数の並び**がこれで決まる。

同時に 2 つ直した:

- メッセージが `true LOR true` だった（`{:?}` で Token を出していた）。上流は `op.tok` の
  `String()`＝`||`。`Token::as_str()` が既にあるので `{}` にするだけ。
- 重複判定と表示に構造キー（`(a EQL 1)` 形式）を使っていた。上流は `astutil.Format`
  ＝ `go/printer` 出力を**キーにも本文にも**使う。`guff::printer::fprint` に差し替えた。
  fixture が `true || true` しか持っていなかったので**差分に出ていなかっただけ**。
- `split` が畳んだ `BinaryExpr` を `seen` に記録していなかった。`a || a || a` で
  外側と内側の両方から報告して 3 件になる（上流は 2 件）。

`no_effects` も `typesinternal.NoEffects` の写しに置き換えた（旧実装は Ident /
BasicLit / SelectorExpr と単純な比較しか通さない過剰に保守的な近似だった）。

#### 2. `buildtag` — 解析 AST にコメントが無い

`// +build` の「misplaced」系は定義上すべて **package 節より後の**コメントの話だが、
guff の parser は `PARSE_COMMENTS` を付けないと**最初の宣言より後のコメントを捨てる**
（`parser.rs` の `next0`）。したがって guff の buildtag は該当のコメントを**一度も見ていなかった**。
gocritic のコメント系と同じ扱い（`PARSE_COMMENTS` で再パース＋`remap_reparsed_pos`）に直した。

ついでに `guff-govet/src/buildconstraint.rs`（手書きの近似）を削除し、
**既に存在していた** `guff::constraint`（`go/build/constraint` の完全移植）に載せ替えた。
近似の側には 2 つの誤りがあった:

- `is_plus_build_line` が `starts_with("// +build")` だったので **`// +buildlinux` を
  正当な +build 行として受理**し、`possible malformed +build comment` を出せなかった。
- `is_go_build_line` が `// go:build`（空白入り）も受理していた。上流の
  `constraint.IsGoBuild` は受理しない。

さらに未実装だった `finish()` の相互検証（`+build lines do not match //go:build condition`）と
`checkOtherFile`（`.s` などの非 Go ファイル）を移植した。

**上流の "malformed //go:build line (space between // and go:build)" は Go ソースから
到達不能**である。`comment()` が `strings.Contains(text, "//go:build")` で分岐するので、
空白入りの `// go:build` はそもそも `goBuildLine` に届かない。fixture
（`buildtag/spaced.go`）を negative 例として置いて、golangci-lint が実際に何も出さないことを
ゴールデンで固定した。

#### 2b. `directive` — 同じ欠陥の 2 例目

`buildtag` を直したあとに `//go:debug` を package 節の後ろに置いた fixture を足したら、
**同じ理由で** guff が黙った（解析 AST にそのコメントが無い）。同じ手当て（再パース＋remap）を入れ、
ついでに未実装だった 2 つを移植した:

- `invalid space %#q in %s directive` — 動詞の直後の空白が `' '` / `'\t'` / `'\n'` **以外**の
  `unicode.IsSpace` だと報告する。guff は `split_whitespace()` で動詞を切っていたので
  区別自体を持っていなかった。`%#q` の描画は実測で確定させた（`'\v'` / `' '`）。
- `nonGoFile`（`.s` などの非 Go ファイル）。

**この 2 つは「同じ根の欠陥が複数の analyzer に散っている」典型**なので、
コメントを見る analyzer を今後追加・移植するときは、まず
「解析 AST にそのコメントは載っているか」を疑うこと。現在この再パースを持つのは
gocritic（コメント系）/ goheader / buildtag / directive / inline。

#### 3. `sigchanyzer` — 条件が反転し、`findDecl` が動いていなかった

上流は

```go
case *ast.CallExpr:
    // Only signal.Notify(make(chan os.Signal), os.Interrupt) is safe,
    // conservatively treat others as not safe, see golang/go#45043
    if isBuiltinMake(pass.TypesInfo, arg) {
        return
    }
```

と、**`make` を直接渡す形だけを免除**する。guff はその形**だけを報告**していた。
そして本来報告すべき `c := make(chan os.Signal); signal.Notify(c, …)` は
`find_decl_rhs` が壊れていて出せなかった:

1. 関数本体を走査する分岐が `let ... GenDecl(gd) = decl else { continue }` の**配下**にあり、
   到達不能だった。
2. 宣言の探索が**使用側 Ident の node id と宣言側 Ident の node id** を比較していた。
   別ノードなので決して一致しない。上流は `ast.Object` の同一性を使う。guff での対応物は
   型検査器の `ObjectId` なので `Info.Defs` で照合するように書き直した。

つまり `Ident` の腕は**一度も発火していなかった**。4 形（`:=` / `var` / 直接 `make` /
関数呼び出し）を実際に golangci-lint に食わせて確定させ、4 形とも一致することを確認した。

#### 4. 報告位置 11 件

上流はすべて `ReportRangef(node, …)`＝ノード自身の開始位置。
`composites`（`{` → CompositeLit）/ `defers`・`errorsas`・`unusedresult`（`(` → callee）/
`nilfunc`（演算子 → 左辺）。gocritic・staticcheck で潰したのと同じクラスの 3 回目。

`printf` だけは別物で、上流は **`%v` という部分文字列の位置**を報告する
（`opRange` → `astutil.RangeInStringLiteral`）。デコード済み文字列でのオフセットを
リテラル**ソース**の位置へ写す必要があるので、エスケープ列を数える
`pos_in_string_literal` を移植した。`"\t%d"` は `%` がデコード後 1 バイト目・
ソース 3 バイト目にある。`call needs N args` だけは `ReportRangef(call, …)` なので callee のまま。

#### 5. その位置写像が、共有ヘルパの文字列デコードのバグを暴いた

移植した位置写像を実際に踏ませるため `printf/escapes.go` を足したところ、
`fmt.Printf("\x41\101%z", 1)` だけ位置が合わなかった。原因は printf 側ではなく
**`guff_analysis::code::unquote_go_string`**（`expr_to_string` 経由で **約 40 か所**が使う共有ヘルパ）で、

```rust
other => other,   // ← バックスラッシュを捨てて次の 1 文字をそのまま積む
```

つまり `\n` `\t` `\"` `\\` の 4 つしか知らず、**`"\x41"` は `x41`、`"\101"` は `101`、
`"\a"` は `a`、`"\u00e9"` は `u00e9`** にデコードされていた。**値も長さも間違っている**。
Go のエスケープ全種（`\a\b\f\n\r\t\v\\\'\"` / `\xHH` / `\OOO` / `\uHHHH` / `\UHHHHHHHH`）を
バイト列として組み立てる形に直した（`\xHH` と `\OOO` は**バイト**であって rune ではない）。

**これは printf 固有の欠陥ではない。** 文字列定数の値を見るチェックすべてに効く。
それでも既存のどのゲートにも出ていなかったのは、**比較しているのがメッセージ本文と行だけ**
だったからで、`%v` の**列**を要求して初めて長さの食い違いが観測可能になった。
§1 が「column を一切比較していない」と書いた穴の、3 回目の実例。

**fixture 側で見つかったもの**

実 toolchain では 2 ファイルがコンパイルできなかった（stub 型検査は通っていた）:
`assign/ok.go` の `declared and not used: x`、`inline_exp/bad.go` の
`package main` に `func main` が無い。**stub が緩いと fixture が現実の Go から乖離する**
という 2026-08-08 と同じ一般則。`composites` の `import "other"` は
モジュール内で解決できる名前に直した。

**golden に載せられないもの**（`sources.txt` に理由を明記）

| 対象 | 理由 |
|---|---|
| `cgocall` | `import "C"` に cgo と C コンパイラが要る |
| `framepointer` | `build.Default.GOARCH` で分岐する＝arm64 の開発機と amd64 の runner でゴールデンが変わる |
| `inline_exp` | `golang.org/x/exp` の解決に第 2 モジュール＋`replace` が要る |
| `inline_ioutil` | メッセージに Go のバージョンが入る（`declared using go1.26.2`）。§5 の 7 番と同じ環境差 |
| `buildtag/bad.go` | `//go:build` 2 行は**ロードエラー**なので golangci-lint は typecheck 失敗を出して他の finding を全部落とす |

前 2 者は台帳の `never` に残る（§6 に追記）。

**結果**

- golden: `govet` **74/74 完全一致・ratchet なし**。7 ケース全部が gate 通過。
- 台帳: govet `never` 16 → **2** / `unit-only` 2 → **0**。
  全体 `never` 23 → **9**、`fired` 420 → **436**（79.7%）。
- `compat/coverage.py` の govet ID 抽出を修正（§3 参照）。
- `docs/COMPATIBILITY.md` の govet 行は「29/29 pass」と書いてあったが、上流は 46 pass で
  guff は 30。未実装 16 個を列挙する形に直した。

**次にやること**

0. **regress `--profile full` の wall ゲート**（4 セッション連続で残っている）。
1. **SA1000（`regexp/syntax`）と SA1001（`text/template`）**。前セッションの見積もりのまま。
2. **SA9008 の IR 検証** / 3. **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
3. **revive の `unit-only` 83 件**。fixture はあるが `stub/dot` のように実 Go では
   解決できない import path があるので、`composites` でやったのと同じ手当てが要る。
   `guff-revive/src/rules/{exported,package_comments}.rs` と `guff-style/src/lll.rs` の
   **行だけの位置写像**もここで露見するはず。
4. **`cargo test --workspace` を CI に載せる**（前セッションの 6 番のまま）。
5. govet の未実装 16 pass（`nilness` / `shadow` / `testinggoroutine` あたりは実コードで
   よく効く）。載せるときは `compat/golden/cases/govet/config.yml` の `enable` に足すこと。
6. `buildtag` / `directive` の **`pass.IgnoredFiles`**（build constraint で除外された `.go`）。
   上流は除外ファイルも再パースして検査する。`pass.ignored_files()` は既にあるので配線するだけだが、
   **golangci-lint 側が本当に同じ集合を渡しているかを確かめる fixture が無い**まま入れると
   OSS で偽陽性になりうるので、先に確かめること。

### 2026-08-10 — revive 99 rule をゴールデン化（`unit-only` 102 → 21）

**やったこと**

7 セッション持ち越されていた「revive の `unit-only` 83 件」を消化した。
`compat/golden/cases/revive/` を新設し、guff が実装する 100 rule のうち **99 を明示的に有効化**して
ゲートに載せた（`enable-all-rules` は使わない。理由は govet ケースと同じ）。fixture は新規に
書いていない — `crates/guff-revive/tests/testdata/revive/` を `sources.txt` が指すだけ。

**載せる前に fixture 側で 2 つ直した**

1. `stub/dot` と `stub/badalias` を `stub/example.com/revive/{dot,badalias}` へ移動し、
   fixture の import path をモジュールで解決できる名前にした。前セッションが予告していた作業。
   ついでに `example.com/badalias` を import しながら stub が `badalias` として登録されていた
   （＝ Rust 側でも解決できていなかった）ズレも消えた。
2. `extended_bad_test.go` を `extended/util/` から**独立したディレクトリ**へ出した。
   上流の `package-naming` は `alreadyCheckedNames.AddIfAbsent(fileDir)` で
   **ディレクトリ単位にメモ化**するが、revive はパッケージ内のファイルを**並行に**lint するので、
   どのファイルがメモを取るかは**レース**になる。3 ファイルのパッケージで実測すると
   3 連続の実行で報告先ファイルが変わった。1 ディレクトリ 1 ファイルにして初めてゴールデンが
   再現可能になる（regen を 3 回回して同一を確認済み）。

**91 rule が発火し、ゲートに載せた瞬間に 187 件の差分**（283 件中 188 一致）。**現在 288 件中 276 一致**。

| 種別 | 件数 | 内容 |
|------|-----:|------|
| **worker panic** | 1 | `inefficient_map_lookup.rs:63` が `for range m {}`（key なし）で `expect("range key")`。**そのワーカーの findings が丸ごと落ちていた**ので bad.go の 43 件が全部消えていた |
| 報告位置 | 約 45 | 4 回目の同じクラス。ただし今回は**逆向き**が多い（guff が名前、上流が宣言の頭） |
| precision | 30 | 大半は `unhandled-error`（下記）と `unexported-naming` |
| メッセージ本文 | 約 20 | 書式・型の描画・上流の言い回し |
| 設定引数の形 | 8 | 下記 |

#### 1. `unhandled-error` — 上流は importer が壊れているので**他パッケージへの呼び出しを見ていない**

guff は `fmt.Print` / `errors.New` に 22 件撃っていた。上流は**0 件**。原因は revive の型検査:

```go
config := &types.Config{ Error: func(error) {}, Importer: importer.Default() }
```

`importer.Default()` は **gc の export data importer** で、いまの Go では stdlib の `.a` を
見つけられない。したがって import は全部 invalid になり、`w.pkg.TypeOf(fCall)` は
`errors.New(…)` に対して `error` でも tuple でもない invalid を返す → 黙る。
**同じパッケージ内で宣言された関数の呼び出しだけが上流に見えている。**

guff は全プログラムの型情報を持っているので、この境界を**手で引き直す**必要がある
(`callee_is_local`)。上流の挙動を fixture で固定するため
`extended_bad.go` に「同一パッケージの `func localError() error` を文として呼ぶ」形を足した
（メッセージの描画も上流の `funcName` に合わせた: selector なら `FullName()` から
`(`・`)`・`*` を除去、それ以外は `go/printer` 出力＝裸の識別子）。

**この「上流の型情報が届かない」クラスは他の rule にも残っている**
（`time-equal` / `epoch-naming` / `range-val-address` の extra がこれ）。ratchet の `why` に列挙した。

#### 2. `function-length` — 上流の `return nil` は `continue` の書き損じ

```go
emptyBody := body == nil || len(body.List) == 0
if emptyBody { return nil }
```

`Apply` はファイル単位なので、**空の関数が 1 つあるとそのファイルの function-length が全部黙る**
（しかも収集済みの failure ごと捨てる）。`extended_bad.go` は上の方に `func badWaitGroup(...) {}`
を持つので上流は 1 件も出さない。上流がそう振る舞う以上そのまま移植した。
なお guff はこの rule を shared_walk のノード走査でも回していたため、
ファイル単位の判断ができるよう `on_file` へ移した。

#### 3. 設定引数の形が上流と違い、**書ける config では rule が黙っていた**

| rule | 上流 | guff（修正前） |
|---|---|---|
| `imports-blocklist` | 引数は**平坦な文字列の並び** | 引数 0 が**リスト**であることを要求 |
| `banned-characters` | 同上 | 同上 |
| `file-length-limit` | `[{ max: 350 }]` の**k,v マップ** | 引数 0 が**整数** |

上流は逆の形を **error にして起動を止める**ので、ユーザーが実際に書ける config は
guff 側で 1 件も効いていなかった（＝ rule が存在しないのと同じ）。
`imports-blocklist` の 6 件はこれ。**Phase 4（設定セマンティクス）の前哨**にあたる欠陥で、
golden tier が config を実際に食わせて初めて出た。

#### 4. `comments-density` — 解析 AST にコメントが無い、の 6 例目

guff は全ファイルを「コメント 0 行」と数えていた（doc コメントだけが AST に残るため）。
`PARSE_COMMENTS` で再パースする形に直した。§4（2026-08-09 4 本目）が
「コメントを見る analyzer はまずこれを疑え」と書いたとおりの再発。
書式も `%2.f%%`（幅 2）に合わせた — `density of  0%` と空白が 1 つ多い。

#### 5. `unexported-naming` — 上流はパッケージレベルを見ない

上流が辿るのは FuncDecl / FuncLit の引数・結果、`:=`、そして**関数本体の中の** `DeclStmt` だけ。
guff は `ValueSpec` を全部見ていたのでパッケージレベルの const / var まで
「the symbol X is **local**」と報告していた（7 件）。上流の `gd.Specs[0]` しか見ない癖も再現した。

#### 6. `multiline-if-init` は**ピン先の revive に存在しない**

revive **v1.15.0**（golangci-lint 2.12.2 の pin）には無く、master にだけある rule。
config に書くと golangci-lint は `cannot find rule: multiline-if-init` で**起動に失敗する**。
つまり guff の `enable-all-rules: true` は上流が出しえない findings を出していた。
`config::AHEAD_OF_PIN_RULES` を新設して `all_rules()`（＝ enable-all の集合）から外した。
明示的に名前を書けば動くのは据え置き。**上流が revive を上げたらここへ戻すこと。**

**結果**

- golden: revive **276/288**（ratchet missing 12 / extra 22）。他 7 ケースは据え置きで緑。
- 台帳: revive `unit-only` 83 → **2** / `fired` 16 → **97**。
  全体 `unit-only` 102 → **21**、`fired` 436 → **517（94.5%）**、`never` 9 は変わらず。
- isolate 114 target すべて一致。`cargo test -p guff-revive` 緑。
- `compat/golden/run.sh` の `sources.txt` パーサを 2 スペース以上区切りに変えた
  （`bad file.go` のように**ファイル名に空白がある** fixture があるため）。

**次にやること**

0. **regress `--profile full` の wall ゲート**（5 セッション連続で残っている）。
1. **revive の ratchet を 0 に**。残りのクラスは `cases/revive/ratchet.json` の `why` に列挙してある。
   最初の一手は **column 0 の表現**（`line-length-limit` / `file-length-limit` は
   `token.Position{Column: 0}` を手で組む）。`Diagnostic` に列の上書きを持たせる必要があり、
   guff-analysis の API 変更になるので、他に column 0 を使う上流 rule が無いか先に調べること。
2. **SA1000（`regexp/syntax`）と SA1001（`text/template`）** — 見積もりは 2026-08-09（3 本目）のまま。
3. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
4. **`cargo test --workspace` を CI に載せる**（3 セッション連続で未着手）。
5. govet の未実装 16 pass。
6. revive の残り `unit-only` 2 件と `never` 1 件（`time-naming`）。

### 2026-08-10（2 本目）— revive の ratchet を 34 → 4 に落とし、上流ソースを一次資料にした

**やったこと**

前セッションが残した 34 件（missing 12 / extra 22）を **4 件（missing 1 / extra 3）**まで潰した。
残り 4 件は全部**同じ 1 クラス**で、しかもポーティングではなく**製品判断**の問題（後述）。

**方法が変わった**: revive v1.15.0 の**ソースが手元にある**ことに気付いた
（`$(go env GOMODCACHE)/github.com/mgechev/revive@v1.15.0/rule/*.go`）。
golangci-lint 2.12.2 がピンしている当のバージョンそのもの。
**推測してから golden で確かめる**のではなく、**先に上流を読んでから直す**形に切り替えたら、
1 件ずつではなくクラス単位で落ちるようになった。以降のセッションもまずここを読むこと。

| クラス | 件数 | 中身 |
|---|---:|---|
| column 0 | 12 | `line-length-limit` / `file-length-limit` |
| confidence の未移植 | 4 | 下記 |
| `enforce-repeated-arg-type-style` | 5 | 報告ノードと results の名前ガード |
| Go 1.22 ゲートの欠落 | 2 | `range-val-in-closure` / `range-val-address` |
| 解析 AST にコメントが無い | 3 | `comment-spacings` / `empty-lines` |
| その他（`empty-lines` の報告ノード、`add-constant` の walk、`package-naming` の `_test`、
`time-date` の表記法、`exported` の doc 判定） | 6 | |

#### 1. column 0 — `Diagnostic` に列の上書きを足した

上流の `line-length-limit` / `file-length-limit` は `token.Pos` から位置を導かず
`token.Position{Line: …, Column: 0}` を**手で組む**。オフセットは 1 始まりなので、
column 0 はどんな `Pos` からも出てこない。`guff_analysis::Diagnostic` に
`column: Option<u32>` を足し、`guff-lint/src/exclude.rs` の `collect_issues` と
`guff-runner/src/cache.rs` の put/get の 2 箇所（＝位置を解決する全箇所）で反映する。
キャッシュ側は `CachedDiagnostic.column_override` を持たせて往復で保存する。
前セッションの宿題「他に column 0 を使う上流 rule が無いか調べる」の答えは
**8 ケースの golden 全体で revive のこの 2 rule だけ**（`grep ':0:' cases/*/expected.golden`）。

`file-length-limit` は行も違った（上流は**最終行**、guff は package 節）。

#### 2. confidence が 1 rule も移植されていなかった

上流は報告地点ごとに `Confidence:` を書き、golangci は `revive.confidence`（既定 0.8）
未満を捨てる。guff は `Failure::confidence()` に exported / var-declaration の
2 例外があるだけで、**残りは全部 1.0** だった。v1.15.0 の `rule/` にある 1.0 未満の
26 箇所を全部 `failure.rs` の表に写した。既定閾値で効くのは 2 つ:

- `optimize-operands-order` = **0.3** — ユーザーに一度も届かない rule だった
- `modifies-parameter` = **0.5** — 同上

残りの 0.8 / 0.9 は既定では通るが、**ユーザーが `confidence` を動かした瞬間に差が出る**。
`empty-block` だけは 2 箇所が**同じ文言**で 0.9 と 1 に分かれるので、
メッセージからは復元できず報告地点で渡している。

なお `crates/guff-revive` の単体テストは「rule が撃つこと」の確認なので、
`extended_test_settings()` の閾値を 0（既定 0.8 ではなく）にして 0.3 / 0.5 の rule も
撃たせ続けている。**既定 config で何が見えるかは golden tier の担当**。

#### 3. `enforce-repeated-arg-type-style` — 報告ノードは「前の」フィールド

上流の `Node` は `prevType`、つまり**省略される側**である直前フィールドの型。
guff は繰り返した側に付けていた。さらに results の分岐にだけ
`field.Names != nil` のガードがあり、`func f() (int, int, int)` は
（名前が無いので型を落としようがなく）**上流は撃たない**。params 側にこのガードは無い。

#### 4. Go 1.22 ゲート — `range-val-in-closure` と `range-val-address`

どちらも冒頭に `if file.Pkg.IsAtLeastGoVersion(lint.Go122) { return }` がある。
1.22 以降はループ変数が毎回別物なので、捕捉もアドレス取得もバグではない。
guff は両方とも無条件に撃っていた。`util::go_version_at_least(pass, 1, 22)` は
`datarace` が既に使っていたものをそのまま使う。
**前セッションの ratchet はこの 2 件を「importer 盲目」と誤分類していた** — 実際は無関係。

#### 5. コメントが解析 AST に無い、の 7 例目と 8 例目

`comment-spacings` は `file.comments` を舐めるだけなので、**本番で 1 件も撃っていなかった**
（doc コメントすら `file.comments` には入らない）。`empty-lines` も同じ理由で
「ブロック先頭のコメント」が見えず false positive を出していた。
両方 `PARSE_COMMENTS` 再パースに寄せた。

このパターンは 4 つの rule に**同一の private コピー**があったので、
`util::reparse_with_comments` 1 本にまとめた。再パースは**私有 `FileSet`** を持つため
位置がそのままでは使えない。`comment-spacings` は報告位置がコメント自身なので、
バイトオフセットを橋にして写す `util::map_reparsed_pos` を足した。

#### 6. 残りの単発

- `empty-lines`: 上流は start / end の**どちらも `Node: block`**。末尾の指摘も開き括弧に出る。
- `add-constant`: 上流は `CallExpr` を見たら**自前で引数だけ調べて `return nil`**、
  つまり呼び出しの部分木に降りない。`go func() { result = 1 }()` の `1` は上流には見えない。
- `package-naming`: `_test` を剥がすのは**規約チェック（下線 / MixedCaps）だけ**。
  bad-name の照合は**フルの名前**を小文字化する。`util_test` は `util` ではない。
- `time-date`: 10 進以外の表記（8 進 / 16 進 / 2 進 / float / 指数 / `1_0`）を
  guff は**黙って捨てていた**。上流はここで
  「use decimal digits for time.Date … 」を出す。`parseDecimalInteger` を移植した。
- `exported`: 上流の `checkGoDocStatus` は OK / Missing / CaseMismatch /
  FirstLetterMismatch / **Unexpected** の 5 値。guff は「大文字小文字違いの前方一致」
  しか見ておらず、**名前に全く触れていないコメント（Unexpected）を見逃していた**。
  5 値と `correctionHint` を移植し、報告位置も上流に合わせて doc コメントに変えた。

**残り 4 件 — 「追従しない」で決着 `[決定 2026-08-10]`**

`context-keys-type`（文言）/ `time-equal` / `epoch-naming`（どちらも extra）。
根っこは 1 つで、revive は `types.Config{Importer: importer.Default()}` で型検査する。
`importer.Default()` は gc の export data importer で、いまの Go には `.a` が無いので
**import は全部 invalid になる**。よって「別パッケージで宣言された型」を要る rule は
上流では全部黙る。guff は全プログラムの型情報を持つので正しく答えてしまう。

0 にするには上流の欠陥をわざと再現して真陽性を捨てることになり、
`time-equal` / `epoch-naming` が**丸ごと死ぬ**。**真陽性を優先し、互換性の方を捨てる**と決めた。
詳細と `unhandled-error` だけ例外にしてある理由は §6 に書いた。
**ratchet の 1/3 は到達目標ではなく固定の床**で、これ以外の差分が増えたらバグ。

#### 7. regress ゲートが `comment-spacings` の偽陽性 10 件と性能退行を捕まえた

**このセッションで唯一、golden では出ず regress で出た欠陥。**
prometheus は `comment-spacings` を有効にしているので、死んでいた rule を生き返らせた瞬間に
`guff_only` が 0 → **10** に増えた。中身は全部 `/* … */` の**単一行ブロックコメント**:

```go
0xEF53: "EXT4_SUPER_MAGIC", /* May also be EXT2_SUPER_MAGIC. */
```

上流は「`/*` で始まり 3 文字目が改行」なら抜け、**そのあと改行でなくてもスペース/タブ判定を
行/ブロックの区別なく適用する**。guff は 2 番目の判定を `else if` に置いていたため、
ブロックコメントには一度も適用されなかった。ついでに allowList も直した:
上流の許容は**引数由来のリスト**と `directiveCommentRE`
（`^//(line |extern |export |[a-z0-9]+:[a-z0-9])`）だけで、guff が持っていた
`//nolint`（コロン無し）/ `//sys ` / `//#nosec` のハードコードは**上流には無い**。

**性能**: 同じ regress が wall の退行も出した。prometheus `./...` を A/B（順序をローテーション
した paired 比較）で測ると **base 比 +0.059s（+3.1%）**。原因は再パースで、
`comment-spacings` を config から外すと差が +0.016s まで落ちる。2 手打った:

| 手 | 中身 | 効果 |
|---|---|---|
| 再パースのキャッシュ | `util::reparse_with_comments` をパッケージ単位でメモ化。**6 rule が同じファイルを個別に再パースしていた**（prometheus では blank-imports / exported / comment-spacings の 3 つが同時に有効） | +0.059 → +0.027s |
| スキャナ化 | `comment-spacings` はコメント本文しか要らないので AST を作らない。`util::scan_comments` が `SCAN_COMMENTS` で 1 回走査し、位置は pass の `FileSet` に写して返す | +0.027 → **−0.011s（base より速い）** |

最終形は 8 ペアの paired 比較で **7/8 で base より速い**（median −0.011s）。
死んでいた rule を生き返らせた**うえで**base より速くなったのは、キャッシュが
base も払っていた重複再パース（blank-imports と exported）を消したから。

**教訓**: `--profile full` は wall ゲートが赤で「判定不能」と 6 セッション書かれていたが、
**`guff_only` の方は生きていて、golden が通した欠陥を捕まえた**。
wall も、ベースラインとの絶対比較ではなくバイナリ 2 本の paired 比較にすれば十分に判定可能。

**結果**

- golden: revive **287/288**（ratchet missing 1 / extra 3）。他 7 ケースは据え置きで緑。
- regress `--profile full`: `guff_only` 0 / `golangci_only` 0 / P=R=100%。
  wall はベースライン 2.330s に対し 2.53s で**赤のままだが、これは 6 セッション前からの
  マシン差**（base バイナリも同じマシンで 2.50〜2.53s）。本セッションの変更は base より速い。
- isolate 114 target すべて一致。`cargo test --workspace` 緑。
- `Failure` に `..Failure::default()` を導入（142 箇所）。以後フィールド追加で
  全報告地点を触らずに済む。`Diagnostic` 側も同様に 36 箇所を関数更新構文に寄せた。

**次にやること**

0. **regress `--profile full` の wall ゲートのベースライン取り直し**（7 セッション連続）。
   絶対値の赤はマシン差なので、**base バイナリとの paired 比較**（順序ローテーション、
   8 ペア）を回せば退行は判定できる、というのが今回の実測。手順は §4 の本エントリに書いた。
   ベースラインをこのマシンで測り直すか、ゲートを paired 比較に作り替えるか。
1. ~~revive の残り 4 件の方針決め~~ → **決着（§6）。追従しない。ratchet 1/3 が恒久的な床。**
2. **SA1000（`regexp/syntax`）と SA1001（`text/template`）** — 見積もりは 2026-08-09（3 本目）のまま。
3. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
4. **`cargo test --workspace` を CI に載せる**（4 セッション連続で未着手）。
   このセッションでローカルは緑を確認済み。
5. govet の未実装 16 pass。
6. **`add-constant` が config を一切読まない**（`allowList` / `maxLitCount` / `ignoreFuncs`）。
   今回 walk を直したときに判明。Phase 4 の材料。
7. revive の残り `unit-only` 2 件と `never` 1 件（`time-naming`）。

### 2026-08-10（3 本目）— SA1001 を brace 数えから `text/template` の移植に置き換えた

**やったこと**

前セッションの「次にやること 2」のうち **SA1001 を完了**。stdlib 近似で残るのは
**SA1000（`regexp/syntax`）1 つだけ**になった。staticcheck-sa の ratchet は
**missing 13 / extra 13 → 12 / 12**。

#### 0. 旧実装は 3 方向すべてに間違っていた

`sa1001.rs` の `validate_text_template` は `{{` と `}}` を数えるだけの 40 行で、
上流が `template.New("").Parse(s)` を呼んで `err.Error()` をそのまま出すのに対し:

| 方向 | 実測 |
|---|---|
| **文言** | 唯一検出できる形でも `template: {{.Name}} : unexpected "}" in operand` と出していた。上流は `template: :1: bad character U+007D '}'`。**parse 名と行番号が入る位置にテンプレート本文を差し込んでいた** |
| **recall** | 報告対象の形は 12 種あるが、**検出できていたのは 1 種**（波括弧の不均衡）だけ |
| **precision** | `{{`（`unclosed action`）を報告していた。**上流は報告しない**（後述の whitelist 外）。新 fixture の `ok.go:21` で旧バイナリが実際に撃つのを確認した |

#### 1. この族に固有の罠 — 「whitelist は parse エラーの部分集合」

上流は `strings.Contains(err, "unexpected") || strings.Contains(err, "bad character")` の
2 クラスだけ報告する。したがって**「Go と違う場所で止まる」ことも同じくバグ**になる:
Go が `illegal number syntax` で止まるところを歩き続ければ、その先の `unexpected` が
**上流には存在しない finding** として出る。SA1002 / SA1007 には無かった形で、
これがあるので**コーパスは報告対象の 2 クラスではなく全メッセージを突き合わせる**。
`ok.go` にも「whitelist 外のエラーで落ちるテンプレート」を 8 本置いた。

#### 2. オラクル `compat/oracles/gotemplate`

`bodies × wrappers` の格子 + 単発形で **2,013 テンプレート**。うち **1,345 がエラー**で
**78 種の異なるメッセージ**に届き、**561 行が報告対象の 2 クラス**に落ちる。
行の形は他のオラクルと 2 点違う（README に記載）:

- 行頭にセクション名（`letter` / `digit` / `parse`）。rune テーブルを同じファイルに載せるため。
- `parse` 行は **4 列目に `html/template` のエラー**。SA1001 はレシーバの出どころ次第で
  どちらの `Parse` も呼ぶので、テストは**全行で両者の一致を主張する**。
  2,013 行すべてで一致した ＝ **1 つの移植で両方を賄えることを推測ではなく実測で確定**させた。

#### 3. 移植したもの

| モジュール | 中身 |
|---|---|
| `gostd/template.rs` | `text/template/parse` の lexer（14 状態）＋ parser。エラー経路のみ。ノードは**エラー文言が要る分だけ**持つ（term の描画と `IsEmptyTree`） |
| `gostd/fmt.rs` | `fmt.Sscan` の complex 経路。`newNumber` は complex 定数を `fmt.Sscan` に渡すので、`{{0x1+2i}}` は `strconv.ParseFloat: parsing "0x1": invalid syntax`、`{{0b1+1i}}` は `syntax error scanning complex number` になる |
| `gostd/strconv.rs`（追加） | `Unquote` / `UnquoteChar` / `ParseUint` / `ParseInt` / `ParseFloat`。数値は**値が一切表に出ない**（surface するのは `integer overflow` と `illegal number syntax` の 2 文言だけ）ので、必要なのは受理集合と overflow 境界 |
| `gostd/unicode.rs` + `unicode_table.rs`（生成） | `unicode.IsLetter` / `IsDigit` |

**`IsLetter` / `IsDigit` を crate で済ませられない理由は `IsPrint` と同じ**。
Go は自分のテーブルが固定された Unicode バージョンで答えるので、識別子・フィールド・変数の
**終端位置**（＝ `bad character` を出すかどうかの境界）がずれる。`goquote-table` と同じ形で
Go から生成し、**全 rune で** Go の答えと突き合わせている。

**位置づけの細かい罠を 1 つ**: `item.String` の切り詰めは
**条件がバイト長 > 10、切り詰めが rune 10 個**。2026-08-07 の godox と同じ非対称で、
`fmt` の `%.10q` は rune で切るが `len()` はバイトを数える。コーパスに
11 バイト・4 rune のトークンを入れて撃たせてある。

#### 4. コーパスが原理的に捕まえられなかったもの — 再帰の深さ

**このセッションで唯一、オラクルでは出ず自分で探しに行って見つけた欠陥。**
移植は再帰下降なので、深いネストは Rust の固定長スタックを食い尽くす。実測すると
**2 MiB スタックの release ビルドで括弧 1,000 段が abort**した。Go は goroutine
スタックが伸びるので 10 万段でも parse する。手書きのテンプレートはそんな形をしていないので
**コーパスにこの行は永遠に現れない** — オラクルという方法自体の盲点で、§7 に記録した。
`MAX_RECURSION = 250` で打ち切り、2 MiB スレッドで 10 万段を回すテストを常設した。

#### 5. fixture の建て直し

旧 `bad.go` は 1 件・`ok.go` は 1 件だった。上流が報告する **12 形すべて**（`bad character` 2 /
`unexpected` 10。行番号が第 2 行になる形と、`html/template` 側の arm を含む）と、
上の「whitelist 外で落ちる」8 本 + 正常 6 本を `ok.go` に置いた。
`checks_test.rs` の `assert!(messages[0].contains("unexpected"))` は
**brace 数えでも通っていた**ので、SA1007 と同じくメッセージ全文を固定した。

**結果**

- golden 8 ケースすべて緑。staticcheck-sa は **205/205 中 193 一致**（ratchet 12/12）で、
  SA1001 の diff は **missing 1 / extra 1 → 0 / 0**。ok.go の 8 本は 1 件も撃たない。
- 新テスト `tests/gostd_template.rs`（2,013 テンプレート ＋ 全 rune の `is_letter` / `is_digit` ＋ 再帰の深さ）。
  `.github/workflows/compat.yml` の `golden` ジョブに追加。
- `cargo test --workspace` **2,981 件緑**、isolate **114 target 一致**、
  OSS `--tier pr,nightly` 8 target すべて据え置き、`./compat/run.sh` 2 target 一致。
- **台帳（`docs/COVERAGE.md`）の件数は変化なし**（517 / 21 / 9）。SA1001 は元から `fired` で、
  §3 が繰り返し書いているとおり **`fired` は「一致している」を意味しない**。
  今回動いたのは ratchet の側だけである。
- regress は tsdb / full の**両プロファイルとも PASS**。`--profile full` は **8 セッションぶり**で（wall 2.420s ≤ 上限 2.480s、
  `guff_only` / `golangci_only` ともに 0）。ただしこれは**このマシンが空いていたから**で、
  同じバイナリの 1 回目は負荷の下で 2.850s だった。**wall ゲートの赤が退行を意味しない**という
  §4 の 2026-08-10（2 本目）の観察の裏返しの実例で、ベースライン取り直しの必要は変わらない。

**次にやること**

0. **regress `--profile full` の wall ゲートのベースライン取り直し**（8 セッション連続）。
   手順は 2026-08-10（2 本目）の §7。
1. **SA1000（`regexp/syntax`）** — stdlib 近似の最後の 1 つ。`regexp/syntax/parse.go` ≒ 2,000 行で
   SA1001 より一桁大きい。文言は ``error parsing regexp: <ErrorCode>: `<Expr>` `` で、
   **`Expr` がどの部分文字列を指すかまで一致させる**必要がある。進め方は本セッションと同じ:
   `compat/oracles/goregexp` を足し、**移植前に**受理集合の差分を測る。golden 3/3。
2. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
3. **`cargo test --workspace` を CI に載せる**（5 セッション連続で未着手）。
4. govet の未実装 16 pass。
5. **`add-constant` が config を一切読まない**。Phase 4 の材料。
6. revive の残り `unit-only` 2 件と `never` 1 件（`time-naming`）。

### 2026-08-10（4 本目）— SA1000 を `regexp` クレートから `regexp/syntax` の移植に置き換えた

**やったこと**

前セッションの「次にやること 1」。**stdlib 近似は 5 つとも移植になり、この族は終わった**。
staticcheck-sa の ratchet は **missing 12 / extra 12 → 9 / 9**。

#### 0. 移植前の実測 — 4,014 行中 1,987 行（49.5%）しか合っていなかった

`compat/oracles/goregexp` を先に作り、旧実装（Rust `regex` クレート + 手書きの書き換え）を
そのまま走らせて数えた。**この数字を取るのが移植の最初の一手**で、SA1002 / SA1007 / SA1001 と同じ順序。

| 内訳 | 件数 |
|---|---:|
| 一致 | 1,987 |
| **誤検出**（Go は受理するのに撃つ） | **589** |
| **見逃し**（Go は拒否するのに黙る） | **389** |
| 文言違い（どちらも「不正」だが文が違う） | 847 |
| そもそも問い合わせられない（入力が UTF-8 でない） | 202 |

誤検出 589 は「Rust の受理集合が RE2 と違う」1 点に集約される。旧実装はそれを
`{`/`}` の逃がしと `[\w-.]` の逃がしという**手書きの書き換え 2 本**で埋めていたが、
それは caddy と grafana で実際に踏んだ形だけを塞いだものだった。

#### 1. この族の中で SA1000 だけが持つ 3 つの罠

| 罠 | 中身 |
|---|---|
| **`Expr` も一致させる** | 文言は ``error parsing regexp: <Code>: `<Expr>` ``。`Expr` は**サイト毎に違う部分文字列**で、`unexpected )` は正規表現全体、`invalid escape sequence` はエスケープ 2 バイト、`invalid repeat count` は演算子とその被演算子、`trailing backslash` は**空文字列**。Code が合っていて slice が違えば golden は同じように落ちる |
| **木を本当に建てないと出ない Code がある** | `expression too large` は**ノードのサイズ**、`expression nests too deeply` は**高さ**、`invalid repeat count` の一部は `repeatIsValid` による**木の再走査**から出る。字句を舐めるだけの実装ではこの 3 つに到達できない。したがって `factor` の 4 ラウンドまで含めた**パーサ全体**の移植になった |
| **whitelist が無い** | SA1001 は `unexpected` / `bad character` の 2 クラスだけ報告するので、移植が困ったときは「その 2 語を含まない文字列を返せば黙る」という逃げ道があった。SA1000 は `regexp.Compile` が返した error を**全部**報告する。**guff 固有の文字列を返す逃げ道が無い**ので、判定できないときは `CompileResult::Undecided` という**第 3 の状態**を作り、SA1000 側が何も報告しない形にした |

#### 2. オラクル `compat/oracles/goregexp`

atoms × wrappers の格子 + 単発形で **4,014 パターン**。うち **1,439 がエラー**で、
**到達可能な ErrorCode 14 種すべて**に届く（`ErrInternalError` は構造上到達不能、
`ErrInvalidCharClass` は宣言だけで `parse.go` のどこからも返らない）。**202 行は入力が
不正な UTF-8** で、これは `ErrInvalidUTF8` の `Expr` が「不正になった以降の末尾そのもの」だから。

行の形は他のオラクルと 1 点違う（README に記載）: **3 列目が verbatim ではなく hex**。
`Expr` はパターンの生の slice なので、タブでも改行でも UTF-8 でないバイトでもあり得る。
`gourl` のように「必ず quote を通るから安全」とは言えないので、Rust 側はバイトで突き合わせる。

限界の 2 行は意図的に大きい。`maxRunes`（33.5M rune）は **Go が持つ中で最も rune 密度の高い
クラス `\pC`（3 バイトで 1,424 rune）**を 23,564 個並べてようやく跨ぐので、その前後 2 行だけで
ファイルの大半を占める。オラクル側に**「`\pC` は今も 1,424 rune か」「その個数で本当に境界を
跨ぐか」を実パーサに問い合わせる assert** を置いてあるので、Go が Unicode を上げて密度が
変わればコーパスが静かに境界を外すのではなく、生成が落ちる。

#### 3. 移植したもの

| モジュール | 中身 |
|---|---|
| `gostd/regexp.rs` | `regexp/syntax/parse.go` の全体（`syntax.Perl` モードのみ）。ノードはアリーナ + free list で、**Go がポインタを height / size マップのキーにしている**のをそのまま再現する（`reuse` された id が次の `newRegexp` で再利用される順序まで一致させないとキーがずれる） |
| `gostd/regexp_table.rs`（生成・240 KB） | `unicode.Categories` / `Scripts` / `FoldCategory` / `FoldScript` / `CategoryAliases` / `SimpleFold` |

**テーブルを生成する理由が `isprint_table` と 1 つ増えている**。名前の集合は
「`\p{Foo}` が finding になるかどうか」を決め、**range の中身は `p.numRunes` を通じて
`expression too large` の閾値を決める**。前者だけならクレートでも代用できるが、後者は無理。

#### 4. 再帰の上限は **2 つに分けた**（`MAX_FACTOR_DEPTH` / `MAX_WALK_DEPTH`）

SA1001 と同じ「goroutine スタックは伸びる」問題だが、**1 つの数字では成立しなかった**。

- `factor` → `collapse` → `factor` は**共通リテラル接頭辞 1 rune につき 1 段**潜る。
  フレームが太く（debug 実測で **600 段が 2 MiB を溢れさせる**）、しかも
  **下りでは Go 自身の `maxHeight` が効かない**（高さの検査は木を建てる上りで走る）。
- 一方 `calcSize` / `calcHeight` / `Equal` / `repeatIsValid` はフレームが薄く、
  **上限は Go の `maxHeight`（1000）を越えていないといけない**。越えていないと
  `(((…1001 段…)))` が Go では `expression nests too deeply` なのに guff は黙る。

そこで前者 **250**、後者 **2000**。代償は「接頭辞連鎖が 250 段より深いパターンで
Go が撃つ `nests too deeply` を撃たない」ことだけで、**誤検出は増えない**。
実在の交替は接頭辞を数 rune しか共有しないので、踏むのは `a|aa|aaa|…` の形だけである。
なお `a|aa|…` は **n ≈ 8190 を越えると rune 予算の方が先に効く**ので、そこから先は再び一致する
（`tests/gostd_regexp.rs` が 2 MiB スレッドで 3 方向とも固定している）。

#### 5. コーパスを 5 個の変異で殴って、盲点を 1 つ確認した

4,014 行が**一発で全部通った**ので、ゲートの側が壊れていない証拠を取った。
移植に既知のバグを 1 つずつ入れて、コーパスが検出するかを見る:

| 入れた変異 | 検出 |
|---|---:|
| メッセージ文言を 1 語変える（`missing closing ]` → `missing close ]`） | **57 行** |
| `unexpected )` の `Expr` を全体から先頭 1 バイトに縮める | **20 行** |
| `maxHeight` を 1000 → 1100 | **2 行** |
| `appendRange` の隣接マージ（`+1`）を落とす | **1 行** |
| **`\P` の符号反転を無視する（`sign = -1` を消す）** | **0 行** |

`a{1000}` の上限を 1001 に変える変異も試したが**検出されない ―― これは正しい**。
`a{1001}` は境界チェックを抜けても `repeatIsValid` が**同じ Code・同じ Expr** で捕まえる。

最後の 1 つは**本物の盲点**で、しかも直しようがない種類のもの:
**符号（`\p` と `\P`）はクラスの中身しか変えず、SA1000 はクラスの中身を報告しない**。
唯一漏れ出す経路は `p.numRunes` → `expression too large` で、そこに届くには
`\PC` を 2 万個以上並べた行が要る（rune 予算の境界行は `\pC` で既に 280 KB ある）。
**オラクルは SA1000 が観測するものしか観測できない**という、この方法自体の限界の 2 例目
（1 例目は 2026-08-10（3 本目）の再帰の深さ）。実害は無い ――
符号を間違えても**誤った finding は出ず**、非現実的な入力で rune の数だけがずれる。

#### 6. fixture の建て直しが S1007 のバグを 1 件出した

`bad.go` は 3 件しか無かったので、**書ける長さのリテラルで到達できる Code を全部**（12 サイト・20 件）に
建て直し、`ok.go` には**旧実装が誤検出していた形**（caddy の `{…}`、grafana の `[…[…]`、
`\Q…\E`、`[\w-.]`）を並べた。`checks_test.rs` の
`assert!(m.contains("error parsing regexp"))` は**近似時代もずっと通っていた**ので、
SA1001 / SA1007 と同じくメッセージ全文を固定した。

新しい `regexp.MustCompile("\\")` が **S1007** を撃ち、そこで判明:
guff は文言に `regexp.Compile` を**ハードコード**していた。上流は
`m.State["fn"]`（マッチしたシンボル）を差し込むので `MustCompile` を呼べば
`MustCompile` と出る。**新しい fixture が無ければ出なかった差分**で、ratchet が
12 → 10 ではなく 12 → 9 まで落ちたのはこの 1 件のおかげ。

**結果**

- golden 8 ケースすべて緑。staticcheck-sa は **223/223 中 214 一致**（ratchet **9/9**）で、
  SA1000 の diff は **missing 3 / extra 3 → 0 / 0**。
- 新テスト `tests/gostd_regexp.rs`（4,014 パターン ＋ 再帰の深さ）。
- `cargo test --workspace` **2,986 件緑**、isolate **114 target 一致**、
  OSS `--oss --tier pr,nightly` 8 target すべて据え置き。
- `guff-staticcheck` から **`regex-syntax` 依存が外れた**（`guff-style` の gocritic 2 check は今も使う）。
- **台帳（`docs/COVERAGE.md`）の件数は変化なし**（517 / 21 / 9）。SA1000 は元から `fired`。
  §3 が繰り返し書いているとおり **`fired` は「一致している」を意味しない**。
- **「次にやること 3」を消化**: `.github/workflows/compat.yml` に `unit` ジョブ
  （`cargo test --workspace`）を追加した。5 セッション連続で先送りされていたもので、
  **golden / isolate が駆動した修正はすべてここに assertion として着地しているのに、
  CI では誰も走らせていなかった**。`gostd_regexp` も stdlib differential のステップに追加。
- regress は **tsdb PASS**（wall 0.850s ≤ 上限 0.880s）、**full は wall だけ FAIL**
  （2.630s > 上限 2.480s）。**finding は両プロファイルとも完全一致**
  （tsdb 4/4、full 20/20、`guff_only` / `golangci_only` ともに 0）なので、
  赤いのは wall ゲート 1 本だけである。
  なお同じ tsdb を負荷の下で回した 1 回目は 0.940s で落ちており、続く 2 回は
  **ハーネス自身の perf-guard が `load average 2.90 > 2.50` と
  「cargo/rustc が動いている」で計測を拒否した** —— 上の 0.850s は
  guard を満たす静かな状態で取った値である。
  §4 の 2026-08-10（2・3 本目）と同じ現象で、ベースライン取り直しの必要は変わらない。

**次にやること**

0. **regress `--profile full` の wall ゲートのベースライン取り直し**（9 セッション連続）。
   手順は 2026-08-10（2 本目）の §7。**perf-guard が効く静かな状態で取ること**。
1. **Go 文字列定数がバイト列でない**（§7 に新規記録）。`regexp.MustCompile("\xff")` を
   上流は `invalid UTF-8` で撃ち、guff は**何も撃たない**。移植側は正しく、
   落としているのは `guff-constant` の `Value::String(Rc<String>)`。
   SA1000 に残る**唯一の既知の非一致**であり、`gostd::regexp` は
   `compile_bytes` を公開済みなので、直すのは定数層の側。
2. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
3. govet の未実装 16 pass。
4. **`add-constant` が config を一切読まない**。Phase 4 の材料。
5. revive の残り `unit-only` 2 件と `never` 1 件（`time-naming`）。

---

### 2026-08-10（5 本目）— Go の文字列定数をバイト列にした（§7 から 1 件回収）

**やったこと**

前セッションの「次にやること 1」。**§7 に「アーキテクチャの違いで再現できない」として
書いたばかりの項目が、実際には単なる表現の誤りだった** —— という点がこのセッションの主題で、
潰した差分そのものより重い。

#### 0. 直す前に測る

前セッションの記述は「SA1000 が 1 件黙る」だった。実際に `regexp.MustCompile` の 5 形を
書いて golangci-lint 2.12.2 に食わせると、**黙るのは 4 件で、5 件目は間違ったことを言っていた**:

```go
regexp.MustCompile("(\xff")
// 上流: SA1000: error parsing regexp: invalid UTF-8: `<0xFF>`
// guff: SA1000: error parsing regexp: missing closing ): `(ÿ`
```

`regexp/syntax` は**字句を舐めながら** UTF-8 を検査するので、`(` の閉じ忘れより先に
不正バイトに当たる。guff は `\xff` を U+00FF にしていたので、そこを通り抜けて
別の診断に落ちていた。**「見逃し」ではなく「誤検出」でもあった**。

#### 1. 直したのは 1 箇所（`Value::String`）、波及は 5 クレート

`guff-constant` の `Value::String(Arc<String>)` を `Arc<Vec<u8>>` にした。付随して:

| 場所 | 中身 |
|---|---|
| `literal.rs` | `decode_escape` が `Escaped::{Byte, Rune}` を返すようにした。Go の `strconv.UnquoteChar` が `multibyte` フラグを返すのと同じ理由で、`\xff` と `\377` は**バイト**、`\u` / `\U` は**コードポイント**。ついでに `\400`（>255）を Go どおり拒否するようにし、`"\x漢"` で `split_at` が rune 境界を割って panic する経路も消えた |
| `value.rs` | `string_val` が `Vec<u8>` を返す（`constant.StringVal` と同じ）。テキストが要る呼び出し側には `string_val_lossy` を新設。`quote` は `strconv.Quote` どおり不正バイトを `\xNN` で書く |
| `utf8.rs`（新規） | Go の `utf8.DecodeRune` と `[]rune(s)` 変換。**Rust の `from_utf8_lossy` は使えない**: 切り詰められた列に対して Unicode の maximal subpart 規則で U+FFFD を **1 個**返すが、Go は 1 **バイト**につき 1 個返す。`"\xe0\xa0"` で 1 対 2 に割れる |
| `guff-types` | `MapKey::Str` / `CaseKey::Str` が `Vec<u8>` に。`len` と添字境界はバイト長になった |

`string_val` の戻り値型を変えたのは、**呼び出し側 12 箇所を一度ずつ見直させるため**。
lossy が正しい場所（SA1024 は上流が `[]rune(s)` する、printf の書式は `for range` する）には
その理由をコメントに書いた。

#### 2. 測り直したら、SA1000 以外に 4 クラス出た

同じ形の probe を SA1002 / SA1007 / SA1011 / SA1020 / SA5009 / govet printf に広げた。
**前セッションが「未確認」と書いた SA1001 / SA1007 の推測は、当たっていた側と外れていた側がある**:

| check | 何が起きていたか |
|---|---|
| **SA1011** | 「この定数は valid UTF-8 か？」を**Rust の `String` に問うていた**ので、構造上**常に yes**。つまり**一度も発火できない check** だった。台帳（`docs/COVERAGE.md`）でも `never` に入っていて、しかも**その原因がこれだと誰も繋げていなかった**。単体テストは `is_valid_utf8_bytes(&[0xff])` を**直接**呼んでいたので、ずっと緑 |
| **SA1007** | 上流は `%q` で URL を引用するので、メッセージに `\xff` が出る。guff は U+FFFD を引用して `\xef\xbf\xbd` と書いていた |
| **SA1002** | 同じ。`ParseError` は layout と詰まった要素の両方を引用する |
| **govet printf** | 2 つ別々のバグ。(a) verb を**バイト**で読んでいた（上流は `utf8.DecodeRuneInString`）ので `%é` が `%Ã`。(b) 列番号がずれる |
| SA1020 | 差分なし。判定は `:` と数字しか見ず、メッセージは定数 |

**SA1011 は `#[ignore = "SC-D08: guff string literals for \xNN (NN>=0x80) differ from Go byte strings"]`
という形で 1 つだけ残っていた `#[ignore]` の中身そのものだった。** 症状は正しく記録されていたのに、
それが `never` の 1 件と同じものだと結び付いていなかった。**`#[ignore]` の理由文と
台帳の `never` を突き合わせるだけで見つかる**類の穴である。

#### 3. printf の列は「上流のバグを移植する」ことになった

`%d` の位置は `astutil.PosInStringLiteral` が生の literal を歩いて求める。その `walkStringLiteral` は

```go
r, _, rest, _ := strconv.UnquoteChar(raw, quote) // 2 番目の戻り値が multibyte
nextI := i + utf8.RuneLen(r)
```

と、**`multibyte` を捨てて `utf8.RuneLen` で進める**。`\xff` は文字列では 1 バイトなのに
ここでは 2 バイト数えられるので、**上流自身が 1 列手前を指す**。golangci-lint と一致させる
以上こちらも同じ数え方をするしかないので、`escape_lengths` が `\xNN` / `\OOO` に対して
「0x80 未満なら 1、以上なら 2」を返すようにした（理由をコメントに書いてある）。

#### 4. 型検査の側にも出ていた

`"\xff"` と `"ÿ"` は Go では**別の文字列**である。guff は両方 `"ÿ"` にしていたので、

```go
switch s { case "\xff": case "ÿ": }   // guff: duplicate case
var m = map[string]int{"\xff": 1, "ÿ": 2} // guff: duplicate key
```

を**型エラーにしていた**。これは finding 1 件の差では済まない: ill-typed なパッケージは
guff が丸ごと飛ばすので、**そのファイルの findings が全部消える**（Phase 1 のゲートが
数えているのはこれ）。回帰テストを `guff-types` の literals / check_files に置いた。

#### 5. 副産物: エクスポートデータの `from_utf8_unchecked` が消えた

`guff-exportdata` の `string_idx` は、**任意のバイト列から `&str` を作る
`unsafe { from_utf8_unchecked }`** を持っていた（＝Rust としては UB）。これは
`Value::String` が `String` を要求していたことへの逃げで、しかも `big.Int` の
リトルエンディアン仮数を `String` 経由で運んでいたので**外せなかった**。
定数がバイト列になったので `string_bytes_idx` / `Decoder::string_bytes` を足し、
定数と数値ペイロードはバイトで、パスや名前は lossy な `String` で読むようにした。
**依存パッケージが `const C = "\xff"` を輸出している場合も、これでバイトが保たれる。**

**結果**

- probe（SA1000/1002/1007/1011/1020/1024/5009 + printf を 1 パッケージに詰めたもの）は
  **golangci-lint と 16/16 完全一致**（開始時は 6 件差）。
- golden 8 ケースすべて緑。**staticcheck-sa の ratchet は missing 9 → 7**（extra は 9 のまま）。
  govet は 0/0 のまま、新しい 4 件（非 ASCII verb・不正バイト）を含めて一致。
- 台帳: `never` **9 → 8**（SA1011 が抜けた）、`fired` 517 → **518**。
- `cargo test --workspace` **2,998 件緑**（2,986 → 新規テスト＋ `#[ignore]` 解除で +12）。
  isolate **114 target 一致**、OSS `--oss --tier pr,nightly` 8 target すべて据え置き。
- regress は tsdb **PASS**（finding 4/4 一致）、full も **PASS**。
  **10 セッション続いた「次にやること 0」はここで終わった** —— ただし結論は
  想定と逆だった。次節を参照。

#### 6. 10 セッション分の診断が間違っていた（regress full の wall）

`--profile full` の wall ゲートは 2026-08-07 以降ずっと赤く、毎回
「マシンが混んでいるからベースラインを取り直せ」と書き送られてきた。
**静かな状態で A/B を取ったら、その診断は全部外れていた。**

まず**このセッションの変更が悪化させていないこと**を、同一マシン・交互 3 回で確かめた:

| 版 | wall（3 回） |
|---|---|
| HEAD（本セッション前） | 2.490 / 2.480 / 2.530 |
| 本セッション | 2.450 / 2.470 / 2.510 |

**むしろわずかに速い。**次に、ベースライン 2.33s を刻んだコミット（`4d345bb`）を
worktree で建てて同じ機械で測った:

| 版 | wall（3 回） |
|---|---|
| `4d345bb`（2.33s を刻んだ版） | 2.260 / 2.230 / 2.240 |
| HEAD | 2.480 / 2.490 / 2.530 |

**機械は当時より速い。ベースラインは古びていない。**差は本物で、17 コミットの
どこかにある。二分すると **1 コミットに全部乗っていた**:

| コミット | wall |
|---|---|
| `4d345bb` | 2.24 |
| **`7edba5f`**（次のコミット） | **2.46** |
| `487849e` / `2e8ec62` / `2f42435` / HEAD | 2.46 – 2.50（以降ほぼ横ばい） |

`7edba5f` は「型検査の false positive 8 件を直し、SA1019 に第三者の deprecation を
見せる」コミットで、**その commit message 自身が「the regress wall check fails on this
machine … so it is the host, not this change」と書いている**。それが誤りだった。

**では何に使われているのか。** SA1019 の依存スキャンを疑って切ってみたが変わらない。
本当の理由は ill-typed パッケージの数だった:

```
4d345bb: ill_typed 14 パッケージ
HEAD:    ill_typed  8 パッケージ
差分:    promql/parser, scrape, tsdb/chunks, tsdb/encoding, util/zeropool, web/api/v1
```

**2.33s は「6 パッケージを丸ごと解析していなかったから速かった」値である。**
ill-typed なパッケージは `run_despite_errors` でない全アナライザを飛ばす（Phase 1）ので、
当時の guff はその 6 つで findings を落としていた。`7edba5f` がそれを直した結果、
**正しく増えた仕事の分だけ遅くなった**。潰すべき無駄ではない。

したがって**改善策は「最適化」ではなく「ゲートを意味のある状態に戻すこと」**とし、
`--update-baseline` で **2.36s / 3.11 GB** を刻み直した。理由をここに残すのは、
数字を上げるだけの再ベースラインは**次の本物の劣化を隠す**からで、
「なぜ上がってよいのか」が書いていない再ベースラインはやってはいけない。

余白は薄い（限界 2.51s に対し実測 2.36–2.51）。**測るたびに緑とは限らない**ので、
再現する FAIL を見たらまず `scripts/perf-guard.sh` と load を疑い、
それが綺麗なら**今度こそ本物の劣化**として二分すること —— 上の表がその手順である。
tsdb 側は 0.760s（限界 0.880s）で余裕があり、据え置いた。

**教訓**: 「ホストのせい」は**測ってから言うこと**。ベースラインを刻んだコミットを
worktree で建て直して同じ機械で走らせるのに、ビルド 2 分＋計測 1 分しかかからない。
10 セッションぶん先送りされた作業の実体は、その 3 分だった。

**次にやること**

1. **`#[ignore]` と `never` の突き合わせを機械化する**。今回 SA1011 は
   「`#[ignore]` の理由文に書いてある」「台帳で `never`」の両方に出ていたのに、
   2 つが同じものだと気付くのに 1 セッションかかった。`compat/coverage.py` に
   **`#[ignore]` の付いたテストが言及する check ID を別ソースとして出す**だけで、
   次の同型は表の上で並ぶ。残る `never` 8 / `unit-only` 21 に同じ形が無いか、これで洗える。
2. **`compat/oracles/goregexp` の 202 行（不正 UTF-8）が今は end-to-end で通るはず**。
   前セッションは「通るのは移植の側だけ」と書いた。定数層が直った以上、
   **`.go` の fixture 経由でも同じ答えになるか**を確かめる価値がある（今回は 5 形しか見ていない）。
3. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
4. govet の未実装 16 pass。
5. **`add-constant` が config を一切読まない**。Phase 4 の材料。
6. revive の残り `unit-only` 2 件と `never` 1 件（`time-naming`）。

---

### 2026-08-11 — gosec 35 rule をゴールデン化（`unit-only` 21 → 3）と、`#[ignore]` の機械化

**やったこと**

前セッションの「次にやること 1」（`#[ignore]` と `never` の突き合わせ）と、
台帳に残っていた最大の未突合ブロック（gosec の `unit-only` 18 件）。

#### 0. なぜ gosec だったか

`unit-only` 21 件のうち **18 件が gosec** で、その 18 件が持っていた「テスト」は

```rust
assert!(messages.iter().any(|m| m.contains("G301:")))
```

—— §1 が名指ししている形そのもの。しかも fixture は `testdata/gosec/stub/` の
**偽の標準ライブラリ**に対して型検査されていた。golangci-lint と一度も突き合わせていない。

#### 1. fixture は Go では**コンパイルできなかった**

golden 化の最初の一歩（実モジュールに置いて `go build`）で 3 件の型エラーが出た:

```go
_ = des.NewCipher(nil)      // assignment mismatch: 1 variable but des.NewCipher returns 2 values
_ = rc4.NewCipher(nil)
_ = cgi.RequestFromMap(nil)
```

**スタブ側の signature は正しかった**（どれも 2 値を返すと書いてある）。
見逃していたのは **guff の型検査器**で、`_ = f()` の arity 不一致を実装していない。
Rust ハーネスは ill-typed を warning で流すので、誰も気付かない。§7 に記録した。

これは「fixture が guff 経由でしか読まれていないと、実 stdlib の形に依存する
バグは原理的に捕まらない」の実例で、golden tier が実モジュールを作ることの意味そのもの。

#### 2. 初回は **52 件中 0 件一致**。原因は severity だった

golden のキーは `path:line:col:linter:severity:text`。**gosec は golangci が
severity を付ける唯一の linter**で（`convertScoreToString` → `low`/`medium`/`high`）、
他の linter は空。guff はスコア表を**持っていた**（`severity:`/`confidence:` の
フィルタに使う）のに、診断に載せていなかった。`Diagnostic::severity` は
**ツリー全体で書き手が 1 人もいない**フィールドだった。

**この 1 フィールドを見るゲートはここ以外に存在しない。**

#### 3. 位置は 5 度目、しかも今回は**両方向**

| rule | 上流 | guff |
|---|---|---|
| G101 | `AssignStmt.Pos()`（第 1 LHS） | `:=` トークン |
| G104 | ExprStmt = call の Pos（callee） | `(` |
| G108 / G50x | `ImportSpec.Pos()`（`_` があれば `_`） | path リテラル |
| G112 | `CompositeLit.Pos()`（型） | `{` |
| **G122 / G703** | **`(` = go/ssa の `CallCommon.pos`** | **callee** |

前 4 つは「内側のトークンを指していた」いつもの形。後ろ 2 つは**その鏡像**で、
`instr.Pos()` を使う SSA アナライザは go/ssa の仕様上 **Lparen** を指す。
**AST ルールは node、SSA アナライザは Lparen** —— gosec ではこの 2 つが同居している。

#### 4. G602: 上流で**到達不能な分岐**を、guff は唯一通る

`trackSliceBounds` の再帰は `Alloc | Parameter | Slice` で、MakeSlice は入っていない。
guff の移植はそれを**コメント付きで正確に写していた**。ところが:

- go/ssa は `make([]T, 定数N)` を `Alloc *[N]T` + `Slice` に落とす。
  だから上流の入口は Alloc で、再スライスの `X` は**常に直前の Slice**。
  **MakeSlice の腕には一生入らない**（＝意味を持たない）。
- guff は同じソースを **MakeSlice 1 個**に落とす。だから再スライスの `X` は MakeSlice で、
  **上流が絶対に通らない腕だけが guff の通り道**だった。

結果 `s := make([]byte, 10); s = s[:2]; s[4]` が**丸ごと黙る**。
5 形の probe で上流と 2/5 → **5/5** に。

**教訓**: IR が違う移植では、**上流で dead な分岐こそ最初に疑う**。
「上流のとおりに書いた」は、上流と同じ入口を通っている場合にしか成り立たない。

#### 5. G204: `TryResolve` を実装した（golden が見たのは 4 件中 1 件）

guff の G204 は「引数が BasicLit か」だけを見ていた。上流は `resolve.go` の
`TryResolve` を回す。8 形の probe を書いて golangci に食わせると、guff は **4 件過検出**:

| 形 | 上流 | 直す前の guff |
|---|---|---|
| `v := "ls"; exec.Command(v)` | 黙る（Decl が literal） | 撃つ |
| `const v = "ls"` | 黙る（`Obj.Kind != ast.Var`） | 撃つ |
| `v := "ls"; v = os.Getenv(); exec.Command(v)` | **黙る**（Decl だけ見る＝フロー非依存） | 撃つ |
| `func f(name string) { exec.Command(name) }` | 黙る（実行ファイル名の位置の param は除外） | 撃つ |

**`ast.Ident.Obj` は parser のファイル単位の解決**である、というのがここの肝。
同じパッケージの**別ファイル**で宣言された識別子は `Obj == nil` で、
`resolveIdent` はそれを「解決済み」と扱う。guff の型情報はパッケージ全体を見えるので、
そのまま辿ると**上流が黙る所で撃つ**。`gosec.rs` の `FileDecls` が
意図的にファイルローカルなのはそのため。probe は 8/8 一致になった。

#### 6. `#[ignore]` の機械化（前セッションの宿題）

`compat/coverage.py` に `#[ignore]` の付いたテストの**本体ごと**走査して、
そこで名指しされている check ID を台帳の状態と**同じ表に並べる**セクションを足した。

理由文だけを見ても足りない、というのが SA1011 の教訓の中身である:
あの `#[ignore]` の理由は `"SC-D08: guff string literals for \xNN … differ"` で、
**`SA1011` という文字列はどこにも無かった**。ID が出ていたのは本体の側。
だから関数本体を brace matching で取って照合している。
単語が平凡な ID（`tests` / `dupl` / `lll`）は `name:` の描画形を要求して誤検出を落とす。

現在の出力は 1 行だけで、それも `fired`（＝別のゲートが見ている）。
**次に `#[ignore]` を書いた人は、それが `never` なら表の上で赤く並ぶ。**

**結果**

- **golden `gosec` ケースを新設: 54 findings / 54 一致・ratchet なし。**
  35 rule 全部が載っている（G602 用の fixture `g602.go` を新規作成）。
- 台帳: `unit-only` **21 → 3**（残りは revive 2 / golines 1）、`fired` 518 → **536（98.0%）**。
  `never` は 8 のまま（うち 3 件は §6 の恒久組）。
- `cargo test --workspace` **2,999 件緑**（+1: G602 の再スライス回帰テスト）。
- golden 9 ケース全部緑（他ケースの ratchet は据え置き）、isolate **114 target**、
  OSS `--tier pr,nightly` **8 target** すべて据え置き。
- regress tsdb **PASS**、full も **PASS**（wall 2.400s / 限界 2.510s、finding 20/20 一致）。
  最初の測定は 2.660s で赤かった —— その切り分けが次節。

#### 7. full の wall は A/B を取って切り分けた

`--profile full` が 2.660s（限界 2.510s）で赤くなった。前セッションの教訓どおり
**「ホストのせい」と書く前に測った**。まず決定的な事実として、
**prometheus の `.golangci.yml` は gosec を有効にしていない** —— 本セッションの
変更は全部 gosec の中なので、full の経路には 1 行も乗っていない。
そのうえで HEAD を worktree に建てて同一マシンで交互に測った（§4 の 2026-08-10 と同じ手順）:

| 版 | wall（交互 3 回） | 中央値 |
|---|---|---|
| HEAD（`5705ad7`） | 2.370 / 2.420 / 2.400 | 2.400 |
| 本セッション | 2.410 / 2.420 / 2.440 | 2.420 |

差は 0.02s（0.8%）で run 間のばらつきの中、**どちらも限界 2.510s の内側**。
赤かった 2.660s は `cargo test --workspace` の直後（load 5 分平均 4.25）に測ったもので、
perf-guard の 1 分平均は通っていたが実際には冷めていなかった。
**静かな状態で測り直したら PASS。**

`--skip-golangci` を付けると 1 回 1 分弱で回るので、A/B は
**worktree のビルド 2 分＋計測 6 分**で終わる。前セッションが 10 回先送りした作業と同じ規模である。

**次にやること**

1. **`_ = f()` の arity 不一致を guff-types に実装する**（§7）。
   `check_assign.rs` の `assign_vars` / `init_vars` は `r == 1 && l != 1` のときだけ
   `eval_multi` に入るので、`l == r == 1` で右辺が tuple の場合を素通りする。
   Go は `assignment mismatch: 1 variable but f returns 2 values` を出す。
   **ill-typed 判定がずれる = そのパッケージの findings が丸ごとずれる**ので、
   Phase 1 のゲートの土台に当たる。上流のメッセージは callee 名を含む形なので、
   `assign_error` の「単一 call の特別扱い」も要る。
2. **`compat/oracles/goregexp` の 202 行（不正 UTF-8）の end-to-end 確認**
   （前セッションの 2 番。まだ手つかず）。
3. gosec の DEFERRED を golden に載せていく: G304 / G305 / G307 / G601 / G115 など
   未実装分と、G402 の MinVersion / CipherSuites、G104 audit モード。
   **`includes` に 1 行足して regen すれば、その rule の上流の答えがそのまま出る。**
4. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
5. govet の未実装 16 pass。
6. **`add-constant` が config を一切読まない**。Phase 4 の材料。
7. revive の残り `unit-only` 2 件と `never` 1 件（`time-naming`）。

---

### 2026-08-11（2 本目）— `_ = f()` の arity を型検査し、台帳の `never` を 8 → 4 に落とした

**やったこと**

前セッションの「次にやること 1」（arity 不一致、§7）と、
**台帳に残っていた `never` 8 件 / `unit-only` 3 件を 1 件ずつ潰す**。
`never` は 8 → **4**、`unit-only` は 3 → **2**、`fired` は 536 → **541（98.9%）**。

#### 1. `_ = f()` — 効いていたのは `isCall` という 4 行のフラグ

`go build` は 4 形すべてを落とす（go1.26.5 で実測）:

```
_ = two()            assignment mismatch: 1 variable but two returns 2 values
x := two()           同上
var y int = two()    multiple-value two() (value of type (int, error)) in single-value context
var a, b = two(), 1  同上
```

guff はこの 4 形すべてで**エラーを 1 件も出していなかった**。原因は 2 つで、
どちらも go/types の中では隣り合っている。

**(a) `assignVars` / `initVars` の `isCall`。** 上流は `l == r` でも
**右辺が単独の CallExpr なら n:n 分岐に入れない**:

```go
isCall := false
if r == 1 { _, isCall = ast.Unparen(orig_rhs[0]).(*ast.CallExpr) }
if l == r && !isCall { ... n:n ... }
```

guff の条件は `r == 1 && l != 1` だったので、`l == r == 1` は素通りしていた。
`(l != 1 || is_call)` に直すと `multiExpr` に入り、tuple が 2 個に展開されて
`l != r` になり、`assign_error` に落ちる。

**(b) `Checker.expr` の `singleValue` が「DEFERRED」のままだった。**
tuple 値がそのまま単値の文脈を通り抜けていた。ここを入れると、
逆に**tuple が正当に来る 4 箇所**を `raw_expr` に移す必要が出る
（上流も同じ理由で `rawExpr` を呼んでいる）:

| 箇所 | 上流 | 正当な tuple |
|---|---|---|
| `eval_multi` | `multiExpr` | `a, b := f()` |
| `arguments`（引数 1 個のとき） | `genericExprList` の `n == 1` の腕 | `g(f())` |
| `builtins` の引数評価（同上） | `exprList` | `println(f())` |
| `ExprStmt` | `stmt.go` の ExprStmt | `http.Get(u)` を 1 行で捨てる |

**4 番目はワークスペースのテストが出した**。`bodyclose` の fixture が
`http.Get("…")` を 1 行で書いており、`single_value` を入れた瞬間に
そのパッケージが ill-typed になって analyzer ごと落ちた。
上流の分岐表を写すのではなく**「上流はどこで rawExpr を呼んでいるか」を写す**のが正しい、
という形の失敗。

**(c) 副産物: `useLHS` が無かった。** 数の不一致で lhs を評価する 3 箇所が
`self.expr` を使っていたので、`_ = two()` が
`cannot use _ as value or type` を**追加で**吐いた。上流の `use1` は
**blank を明示的に飛ばす**。`use_n` / `use_1` を足し、
`r != 1` の枝も上流どおり「lhs も rhs も無事なときだけ mismatch を報告する」に変えた。

**(d) `eval_multi` の `want == 2` は `allowCommaOk` ではなかった。**
上流は `multiExpr(e, l == 2 && returnStmt == nil)` で、**return では comma-ok を許さない**。
guff は `want`（＝ l）だけを見ていたので、`return m[k]` を 2 値の関数から返すと
comma-ok に展開していた。引数を `allow_comma_ok: bool` に変えた。

**測ったこと**: 効果は finding 1 件ではない。ill-typed はパッケージ単位のスイッチで、

```
package tc: strings.Index(s,"x") > -1  ← S1003
            _ = two()                  ← 型エラー
```

golangci-lint は typecheck エラーだけを出して S1003 を落とす。
**guff は直す前は S1003 を出していた**（＝ユーザーに見える差）。直したあとは両方黙る。
OSS 8 ターゲットの `ill-typed N, at baseline` は 1 つも動かなかったので、
実コードでの偽陽性は無い。

なお **guff は typecheck エラー自体を finding として出さない**（golangci-lint は
`typecheck` 疑似 linter として出す）。これは別件で、ここでは触っていない。

9 形の probe を `go build` と突き合わせた結果、**7 形は位置も文言も完全一致**。
残り 2 形は**どちらも文言だけの差**で、ill-typed の判定は両方とも揃っている:

| 形 | `go build` | guff |
|---|---|---|
| `x := none()` | `none() (no value) used as value`（3:17） | `cannot assign to func() in assignment`（3:17） |
| `g(two())` で g が 1 引数 | `too many arguments in call to g` + have/want（4:14） | `too many arguments in call`（4:12） |

前者は `Checker.expr` の `exclude(x, novalue|builtin|typexpr)` が未実装だから
（`single_value` の隣にある、今回入れなかった半分）。後者は `arguments` のエラーが
callee 名と have/want の 2 行を落としているため。**どちらもゲートには出ない**
（guff は typecheck エラーを finding にしないので）。

`go/types` の `ExprString` を `crates/guff-types/src/exprstring.rs` に移植した。
`assignment mismatch: 1 variable but v.m returns 2 values` の `v.m` と、
`multiple-value two() (…)` の `two()` がこれ。**短縮の仕方まで含めて仕様**
（composite literal の中身は `…`、関数リテラルは `(func() literal)`）なので、
source printer で代用はできない。

#### 2. `S1030` — スタブの受信子が値だったので、port も値で書かれていた

golden が `missing` として挙げていた 1 件。原因は 1 行:

```rust
matches!(name, "(bytes.Buffer).Bytes" | "(bytes.Buffer).String")   // 上流は (*bytes.Buffer)
```

`Bytes` / `String` は `*bytes.Buffer` のメソッドなので上流の
`code.IsCallTo(pass, call.Args[0], "(*bytes.Buffer).Bytes")` とは永久に一致しない。
**なぜそう書かれたかが本題**で、fixture の偽 stdlib が

```go
func (Buffer) String() string { return "" }   // 値レシーバ
```

だった。port は上流ではなく**スタブに合わせて**書かれていた。
これは 2026-08-11（1 本目）の gosec の「実 Go ツールチェインに一度も読ませていない
fixture はこうなる」の 2 例目で、今回は**スタブの側が実物と違う**という形。
スタブをポインタレシーバに直し、上流に合わせて 3 点も直した:

- 型判定は識別子名ではなく `TypeOf(call.Fun)`（`[]byte(...)` の `Fun` は
  `ArrayType` なので、`is_builtin_ident(fun, "[]byte")` は**一度も真にならない**死んだ枝だった）
- メッセージは `report.Render(sel.X)` と `report.Render(call)` を埋める
  （`"buf"` と `"string(buf.Bytes())"` が**ハードコード**されていた）
- `m[string(buf.Bytes())]` は**報告しない**（コンパイラの最適化で
  `m[buf.String()]` より速い）。上流は cursor の親を見るので、guff は
  IndexExpr の子の node id を先に集めた

fixture を 4 形に増やして golden 4/4 一致。`staticcheck-s` の ratchet は missing 3 → **2**。

#### 3. `SA3000` / `SA1027` — 「発火しない」のは fixture ではなく**モジュールと arch**が原因だった

どちらも `never` で、どちらも fixture は最初からあった。

- **SA3000** は `version.Compare(code.StdlibVersion(pass, node), "go1.15") >= 0` で抜ける。
  `cases/staticcheck-sa` の go.mod が `go 1.22` なので上流も guff も黙る。
  **ファイルに `//go:build go1.14` を書いても効かない**: `StdlibVersion` は
  モジュールが 1.21 以上なら**ファイルタグが上回るときしか採用しない**（実測で 0 件）。
  → `go 1.14` のモジュールを持つケース `cases/staticcheck-go114` を新設。
  **1 回目の実行で位置バグが出た**: 上流は `FuncDecl` を報告するので
  `Type.Pos()` = `func` キーワード、guff は関数名を指していた（内側トークン、6 度目）。
- **SA1027** は `sizes.Sizeof(uintptr) != 4` で抜ける。64-bit ホストでは
  どちらも永久に黙る。→ golden ランナーに**ケース単位の `env` ファイル**を足し、
  `cases/staticcheck-386`（`GOOS=linux GOARCH=386`）を新設。
  `GOARCH` だけでは駄目で、`darwin/386` は成立しないので golangci-lint が
  `no go files to analyze` を返す。**GOOS も一緒に動かす**必要がある。2/2 一致。

この `env` の仕組みは §6 が `govet/framepointer` について
「入れれば解ける」と書いていたものだが、**framepointer には効かなかった**。次項。

#### 4. `govet/framepointer` — §6 に書いてあった理由が間違っていた

§6 は「`build.Default.GOARCH` がホスト依存だからゴールデンに載せられない」としていた。
`env` を入れたので試したところ、**`GOARCH` を合わせても 0 件**。
同じ fixture に `go vet` を食わせると:

```
bad/bad_arm64.s:2:1: frame pointer is clobbered before saving
bad/bad_arm64.s:1:1: [arm64] bad1: function bad1 missing Go declaration
（計 6 件）
```

golangci-lint 2.12.2 は**同じ入力に対して 0 件**。ホスト arch のままでも同じ。
つまり **golangci-lint は `.s` ファイルの診断を通さない**（asmdecl も同時に死んでいる）。
GOARCH は無関係だった。§6 の行を実測に書き換えた。
**「入れれば解ける」と書いてある制約でも、入れてから測るまでは解けたことにならない。**

#### 5. `revive/time-naming` — rule が丸ごと死んでいた

`never` の 1 件。原因は 2 つ:

- 名前の型を `Info.Types` から引いていた。ValueSpec の名前は**定義**なので
  `Info.Types` には無い（上流の `Pkg.TypeOf` は `Defs` にフォールバックする）。
  **つまりこの rule は一度も報告を出せなかった。**
- `file.decls` を歩いていたのでパッケージレベルの `var` しか見ていない。
  上流の visitor は `*ast.ValueSpec` を**どこでも**拾うので関数内の `var` も対象。

直すと `var timeoutSec time.Duration` / 関数内の `var deadlineSeconds …` の両方を撃つ。
**上流は両方とも黙る** —— revive の importer 盲目（§6）で `time.Duration` が解決できないため。
方針どおり真陽性を優先し、`cases/revive` の ratchet を extra 3 → **4** にして
§6 の表に 1 行足した。**床が 1 段上がったので、`why` も更新してある。**

#### 6. `revive/forbidden-call-in-wg-go` — `unit-only` の理由はモジュールの Go バージョン

上流は `if !file.Pkg.IsAtLeastGoVersion(lint.Go125) { return nil }`。
`Pkg` なのでバージョンは go.mod 由来で、ファイルタグでは上げられない。
`cases/revive` は `go 1.22`。単体テストの fixture は**モジュールを持たない**ので
「十分新しい」と読まれ、そちらだけが通っていた（＝ `unit-only` の正体）。

`cases/revive` を 1.25 に上げると 290 件の golden で他の版依存 rule も同時に動くので、
`go 1.25` の小さなケース `cases/revive-go125` を新設した。2/2 一致。
**1 回目は severity で割れた**（golden `revive:warning:` / guff `revive::`）。
guff の revive severity は config 由来で、`cases/revive` は `severity: warning` を
書いている。上流も同じで、config に無ければ空。ケースの config に 1 行足して解決。

**結果**

- 台帳: `never` **8 → 4**、`unit-only` **3 → 2**、`fired` 536 → **541（98.9%）**。
  回収したのは `S1030` / `SA1027` / `SA3000` / `revive/time-naming` /
  `revive/forbidden-call-in-wg-go`。
- golden ケース **9 → 12**（`staticcheck-go114` / `staticcheck-386` / `revive-go125`）。
  12 ケース全部緑。ratchet は `staticcheck-s` が 3/1 → **2/1**、
  `revive` が 1/3 → **1/4**（§6 の恒久組が 1 件増えたため）。他は据え置き。
- `cargo test --workspace` **3,011 件緑**（+12: single_value 11 + wg_go 1）。
- isolate **114 target**、OSS `--tier pr,nightly` **8 target** すべて据え置き。
  OSS の `ill-typed N, at baseline` が 1 つも動かなかったのが arity 修正の安全確認。
- regress tsdb **PASS**（wall 0.760s / 限界 0.880s、finding 4/4 一致）、full も **PASS**
  （wall 2.410s / 限界 2.510s、finding 20/20 一致）。次項。

#### 7. wall が 2 回赤くなり、1 回は本物だった

最初の tsdb は 0.940s（限界 0.880s）。**「ホストのせい」と書く前に、まず疑わしい変更を
数えた**: S1030 に足した `IndexExpr` の**全ファイル走査**が、prometheus が
staticcheck を有効にしている以上**全パッケージに乗る**。`m[string(buf.Bytes())]` の
除外にしか要らない走査なので、**候補が 1 件も無ければ走らせない**ように後置きにした
（実コードではまず走らない）。ついでに `time-naming` も、`is_duration_type` が
型を文字列に描画するのに**全変数について**呼んでいたので、
先に接尾辞（ただの文字列比較）で弾くよう順序を入れ替えた。報告集合は変わらない。

直したあと tsdb は 0.760s で **PASS**。full は依然 2.610s で赤かったので、
前セッションと同じ手順で worktree に HEAD を建てて交互に測った:

| 版 | wall（交互 3 回） | 中央値 |
|---|---|---|
| HEAD（`ee56f7b`） | 2.410 / 2.420 / 2.450 | 2.420 |
| 本セッション | 2.410 / 2.420 / 2.480 | 2.420 |

**差 0.00s。** 静かな状態で測り直したら 2.410s で PASS。
2 回目の赤は 3 分前に `cargo build --release` を回した直後のもの。

なお RSS は tsdb で 856 MB（baseline 748 MB の 1.14 倍、限界 1.20 倍）と
限界に近いが、**前セッションの記録が既に 865 MB** なので本セッションの寄与は 2% 程度。
**次に何か足す人は先に RSS の baseline を測り直すこと。**

**次にやること**

1. **`golines` / `swaggo` を isolate に載せる**（台帳の最後の `unit-only` / `never`）。
   golangci-lint v2 でこの 2 つは `formatters:` ブロックなので、
   `compat/isolate/make_config.py` の `TEMPLATE` が `linters.enable` しか書けないのを直す。
   fixture は `compat/isolate/fixtures/{golines,swaggo}/` を新設。
2. **`compat/oracles/goregexp` の 202 行（不正 UTF-8）の end-to-end 確認**
   （3 セッション積み残し）。
3. gosec の DEFERRED を golden に載せていく: G304 / G305 / G307 / G601 / G115 など
   未実装分と、G402 の MinVersion / CipherSuites、G104 audit モード。
4. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
5. govet の未実装 16 pass。
6. **`add-constant` が config を一切読まない**。Phase 4 の材料。
7. **guff は typecheck エラーを finding として出さない**。golangci-lint は
   `typecheck` 疑似 linter として出すので、ill-typed なパッケージでは
   **golangci が 1 件、guff が 0 件**になる。今回 ill-typed の判定は揃えたが、
   出力は揃っていない。golden ケースは typecheck 混入を避ける前提で書かれているので、
   載せるなら専用ケースが要る。
8. `staticcheck-s` の残り 2 件（SA4006 ×2、空 `if` 本体のブロック最適化）と
   `S1037` の extra 1 件。

---

### 2026-08-11（3 本目）— formatter を isolate に載せたら、`linters.default: none` が効いていなかった

**やったこと**

前項の「次にやること 1」（`golines` / `swaggo` を isolate に載せる）。
**1 つの fixture を書いただけで、formatter 全体に効く欠陥が 2 つ出た。**

#### 1. `linters.default: none` は「標準セットを走らせる」と同義だった

`make_config.py` に formatter 用テンプレート（`formatters:` ブロック）を足して
最初の probe を回したところ、guff が `unused` を報告した。切り分けると:

| config | golangci-lint | 直す前の guff |
|---|---|---|
| `linters: {default: none}` | `Running error: no linters enabled` / exit **3** | 標準 5 linter を実行 |
| `default: standard` + 5 つ全部 `disable` | 同上 | 同上 |
| `default: none` + `formatters: {enable: [golines]}` | formatter だけ実行 / exit 1 | golines + 標準 5 linter |

原因は `cli.rs` の 1 行:

```rust
let analyzers = if linter_names.is_empty() && args.enable.is_empty() {
    // 標準プリセットにフォールバック
```

**「設定が空」と「設定が明示的に空」を同一視していた。** 前者は起こり得ない
（設定が無ければ `LinterDefault::Standard` なので `resolve_names()` は空にならない）ので、
このフォールバックは**後者にしか当たらない** —— つまり
**「全部 disable」を「全部 enable」に読み替える**分岐だった。

golangci に合わせて exit 3（`EXIT_NO_LINTERS`）で止めるようにした。ただし
**formatter が 1 つでも有効なら止めない**（`linters.default: none` + `formatters:` は
正当な「フォーマットだけ」設定で、golangci はこれを普通に実行する）。
`run_and_write` の `analyzers.is_empty()` ガードにも同じ条件を足した ——
そちらは `Ok(0)` で早期 return するので、formatter が走らなくなる。

**これは compat の話であると同時に、素の precision バグである。**
`disable` に 5 つ並べたユーザーは、guff から 5 つ全部の findings を受け取っていた。

#### 2. formatter の finding は**ファイルに 1 件**

fixture を isolate に通すと `guff=2 golangci=1`。
guff の `first_changed_lines` は差分の**変更グループごと**に 1 件返す。
`gofmt` で確かめると formatter 共通の欠陥だった:

```go
func one(  ) {}     // ← 3 行目

func two() { … }    // 3 行の context を超える距離

func three(  ) {}   // ← 10 行目
```

`max-same-issues: 0` / `uniq-by-line: false` でも **golangci は 3 行目だけ**。
golangci 自身の golines testdata も、長い行が十数個ある 1 ファイルに対して
`// want +1` が 1 個しか無い。→ `check_files_multi` で
**(formatter, file) ごとに最初の 1 件だけ**を出すようにした。

**gofmt / gofumpt / gci / goimports / golines の 5 つ全部に効く。**
台帳上はどれも `fired` で、既存のゲートを全部通っていた ——
**corpus のリポジトリが整形済みで、2 ヶ所以上ずれたファイルが 1 つも無かった**だけ。

#### 3. `swaggo` だけは載せられない

`swag` バイナリが要る（guff は shell out する）。CI に入れるかは
`golines` と同じ判断になるが、`golines` は golangci が**ライブラリとして**
`v0.15.0` を埋め込んでいるので**同じ版をピンできる**のに対し、
`swag` 側は golangci が `github.com/golangci/swaggoswag` を使っており対応が自明でない。
版がずれた瞬間に整形結果が割れて偽の diff になるので、ピンの根拠が出るまで保留。
CI には `go install github.com/golangci/golines@v0.15.0` を足した（版はピン）。

**結果**

- 台帳: `fired` 541 → **542（99.1%）**、`unit-only` **2 → 1**（残りは
  `multiline-if-init` = 上流の pin に存在しない恒久組）、`never` は **4** のまま
  （3 件は §6 の恒久組、残り 1 件が `swaggo`）。
- isolate **115 target**（`golines` を新設）。golden 12 ケース、OSS 8 target すべて据え置き。
- `cargo test --workspace` **3,013 件緑**（+2: `default: none` の CLI テスト）、
  `compat/tests` **61 件緑**（+3: formatter テンプレートのテスト）。
- regress tsdb **PASS**（0.870s / 限界 0.880s）。full は 2.640s で赤だったが、
  **直前のコミット `46cb255` 自身が同じ条件で 2.520s**（限界超え）を出す状態だった。
  交互 A/B:

  | 版 | wall（交互 3 回） | 中央値 |
  |---|---|---|
  | `46cb255` | 2.420 / 2.470 / 2.520 | 2.470 |
  | 本セッション | 2.420 / 2.490 / 2.570 | 2.490 |

  **差 0.02s（0.8%）で、両版とも回を追うごとに同じだけ上がっていく。**
  本セッションの変更は CLI の分岐 1 つと、formatter finding を**減らす**方向の
  truncate だけなので、遅くなる経路は無い。1 時間前に同じ full が 2.410s で
  緑だったことと合わせて、**ホストの温度**と判断した。
  この機械は本セッション中ずっと外部プロセスで load 3〜6 を維持している
  （perf-guard が `cursor-agent worker present` を警告している）。
  **静かになってから測り直したら 2.360s = baseline ちょうどで PASS。**

**次にやること**

1. **`swaggo`**（台帳最後の `never` のうち唯一到達可能なもの）。
   golangci の `github.com/golangci/swaggoswag` と `swag` CLI の対応版を特定して
   CI にピンできるか調べる。できないなら §6 に恒久組として書く。
2. 以下は 2026-08-11（2 本目）の「次にやること」2〜8 がそのまま残っている。

---

### 2026-08-11（4 本目）— `goregexp` の 202 行を end-to-end で確認し、`S1037` の extra を消した

**やったこと**

3 セッション積み残していた「`compat/oracles/goregexp` の 202 行（不正 UTF-8）の
end-to-end 確認」と、`staticcheck-s` に残っていた extra 1 件。

#### 1. 202 行 — **移植は 202/202 合っている**。残るのは Rust の `String`

オラクルの 202 パターンを全部 `regexp.MustCompile("\xNN…")` として 1 ファイルに
書き出し、**text 出力**で両ツールをバイト比較した（JSON では駄目で、その理由が答え）。

| 観測 | 結果 |
|---|---|
| finding 数 | 202 / 202 |
| file:line:col | 全行一致 |
| メッセージ | **`Expr` の描画以外は全行一致** |

差は 1 点だけで、`syntax.Error.Expr` が「パターンの生 slice」であること
——Go の `string` は持てて Rust の `String` は持てない——に帰着する。
**置換の粒度は Go の `encoding/json` と同じ（1 バイト = 1 個の U+FFFD）**なので、
JSON を通す golden tier では一致し、golangci の text 出力とだけ割れる。
詳細と 13 行の内訳は §7 に書いた。

fixture には golden で区別できる 4 形（`\xc3` / `\xe2\x82` / 末尾に演算子）だけ足した。
202 行を全部足しても golden 上は同じ `` の列が並ぶだけで情報が増えない。

#### 2. `S1037` — fixture が guff の側に合わせて書かれていた

`staticcheck-s` に残っていた唯一の extra。上流のパターンは

```
(SelectStmt (CommClause (UnaryExpr "<-" (CallExpr (Symbol "time.After") [arg])) body))
```

で、guff は 2 箇所ゆるかった。7 形の probe で実測:

| 形 | 上流 | 直す前の guff |
|---|---|---|
| `case <-time.After(d):`（本体空 / 非空） | 撃つ ×2 | 撃つ ×2 |
| `case t := <-time.After(d):` | **黙る**（Comm が `AssignStmt` なので `UnaryExpr` のパターンに当たらない） | 撃つ |
| 別の型の `c.After(d)` | **黙る**（`Symbol` は object を解決する） | 撃つ（セレクタ名 `After` だけを見ていた） |
| clause 2 個 / `default:` 付き | 黙る | 黙る |

**fixture が guff の側に合わせて書かれていた**（`bad.go` は代入形しか持っていなかった）
ので、bad/ok を上流の答えどおりに建て直した。7/7 一致。

#### 3. `guff run` に `-E` / `-D`

golangci はどちらも短縮形を持ち、`guff fmt` の側には既に `-E` があった。
`golangci-lint run -E gosec` をそのまま打つと `unexpected argument '-E'` で落ちる。

**結果**

- `staticcheck-s` の ratchet **2 missing / 1 extra → 2 missing / 0 extra**。
  **この case から「guff が上流より多く撃つ」形が消えた。**
- `staticcheck-sa` の golden に SA1000 の不正 UTF-8 が 4 形増えて 4/4 一致
  （ratchet 7/9 は据え置き）。
- `cargo test --workspace` **3,014 件緑**、`compat/tests` 61 件緑。
- golden 12 ケース、isolate 115、OSS 8 すべて緑。
  regress tsdb **PASS**（0.750s / 限界 0.880s）、full も **PASS**（2.420s / 限界 2.510s）。
  途中 tsdb が 0.940s で 2 回赤くなったが、HEAD（`98dab1f`）と交互に測ると
  **0.760 / 0.760 / 0.760 対 0.760 / 0.770 / 0.760** で差は 0.00s。
  `--skip-golangci` を付けた測定は系統的に速く、赤かったのは
  **直前の golangci-lint 実行で機械が温まっていた**回だった
  （harness は guff を先に測るので同一 run 内の汚染ではなく、前の run の残り熱）。
  本セッションの変更のうち prometheus の経路に乗るのは S1037 だけで、
  しかも**分岐を 1 本減らしている**。

**次にやること**

1. **`swaggo`**（3 本目の 1 と同じ）。
2. gosec の DEFERRED を golden に載せていく: G304 / G305 / G307 / G601 / G115 など
   未実装分と、G402 の MinVersion / CipherSuites、G104 audit モード。
3. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
4. govet の未実装 16 pass。
5. **`add-constant` が config を一切読まない**。Phase 4 の材料。
6. **guff は typecheck エラーを finding として出さない**（2 本目の 7）。
7. `staticcheck-s` の残り 2 件（SA4006 ×2、空 `if` 本体のブロック最適化）。
   **`staticcheck-s` はこれで 0/0 になる。**

---

### 2026-08-11（5 本目）— `swaggo` は「載せていない」のではなく**死んでいた**。台帳の `never` が §6 と一致した

**やったこと**

3 本目・4 本目が「版がピンできないので保留」と書いた `swaggo`。
調べたら**ピンできる**うえに、**guff の formatter が一度も動いていなかった**。

#### 1. 版は readme に書いてある

golangci が使う `github.com/golangci/swaggoswag` は `swaggo/swag` の hard fork で、
readme が同期元のコミットを名指ししている:

```
- sync with 93e86851e9f22f1f2db57812cf71fc004c02159c (after v1.16.4)
```

CI にはこれをピンした（`go install github.com/swaggo/swag/cmd/swag@93e86851e9f2…`）。
guff は CLI に shell out し、golangci はライブラリをリンクするので、
**版がずれた瞬間に整形結果が割れて偽の diff になる**。

#### 2. `swag fmt` は**ドットで始まるディレクトリを飛ばす**

ピンした CLI を PATH に置いても guff は 0 件。切り分けると、
guff は `$TMPDIR/.guff-swaggo-<pid>-<n>` に 1 ファイルだけ置いて
`swag fmt -d <dir>` を呼ぶのだが、上流の `walkWith` が

```go
len(f.Name()) > 1 && f.Name()[0] == '.' && f.Name() != ".."  // exclude all hidden folder
```

で `filepath.SkipDir` を返す（`vendor` と `docs` も同様）。
**staging ディレクトリごと飛ばされ、入力がそのまま返ってきていた。**
先頭のドットを外すだけで直る。

**この formatter は実装された日から一度も整形していない。** 台帳が
`swaggo` を `never` にしていたのは「corpus が使っていないから」ではなく、
**動いていなかったから**だった。

#### 3. 単体テストは「何かした」しか見ていなかった

`formats_swag_comments` は

```rust
assert!(!out.is_empty());
assert!(String::from_utf8_lossy(&out).contains("@Summary"));
```

—— **入力をそのまま返す formatter が両方とも満たす**。§1 が名指ししている形そのもの。
`assert_ne!(out, src)` と「タブで整列されていること」に変え、
staging ディレクトリが hidden でないことを直接見るテストを足した。

**結果**

- isolate **116 target**（`swaggo` を新設、1/1 一致）。
- 台帳: `fired` 542 → **543（99.3%）**、`never` **4 → 3**。
  **`never` の 3 件は §6 の恒久組そのもので、「まだ載せていない」check は無くなった。**
  `unit-only` の 1 件は `multiline-if-init`（pin した revive に存在しない）。
- `cargo test --workspace` **3,015 件緑**（+1: staging ディレクトリが hidden で
  ないことを直接見るテスト。整形テストの方は強化しただけで件数は増えていない）。
  なお整形テストは `swag` が PATH に無ければスキップする ——
  **CI の `tests` ジョブには入れていない**ので、そちらでは実質スキップになる。
  実際に守っているのは isolate の `swaggo` target で、あちらは PATH にピン版を置く。
- golden 12 ケース、OSS 8 すべて据え置き。regress tsdb **PASS**（0.760s）、
  full **PASS**（2.410s）。

#### 4. 計測の余談 —— このホストの wall には太い裾がある

本セッション（1〜5 本目）で regress を 15 回以上回した結果として記録しておく。
**同じバイナリ・同じ load でも、10 回に 1 回くらい中央値の +20〜25% が出る。**
実測（すべて本セッション、コード変更なしの反復）:

| profile | `--skip-golangci` を 3 回 | 公式ゲートの外れ値 |
|---|---|---|
| tsdb（限界 0.880s） | 0.760 / 0.760 / 0.770 | **0.940** が 3 回、直後の再実行は 0.760 / 0.760 |
| full（限界 2.510s） | 2.430 / 2.430 / 2.460 | **2.640** が 3 回、静かにしてから 2.360 / 2.420 |

**外れ値かどうかは A/B でしか判定できない。** 本セッションでは 3 回 A/B を取り、
3 回とも差は 0.00〜0.02s だった。`--skip-golangci` は 1 分弱で回るので、
**赤を見たらまず HEAD と交互に 3 回**というのが一番早い。
限界は tsdb が baseline + 0.150s（+20%）、full が +0.150s（**+6%**）で、
**full の余裕はこのホストの裾より狭い**。

**次にやること**

1. gosec の DEFERRED を golden に載せていく: G304 / G305 / G307 / G601 / G115 など
   未実装分と、G402 の MinVersion / CipherSuites、G104 audit モード。
2. **SA9008 の IR 検証** / **SA5011 の σ 相当**（§7）。consul の allowlist 3 件がこれ。
3. govet の未実装 16 pass。
4. **`add-constant` が config を一切読まない**。Phase 4 の材料。
5. **guff は typecheck エラーを finding として出さない**（2 本目の 7）。
6. `staticcheck-s` の残り 2 件（SA4006 ×2）。これで `staticcheck-s` は 0/0。
7. **台帳は 99.3% で頭打ちになった。次に測るべきは「発火したか」ではなく
   「何件比較しているか」** —— §1 の isolate 178 findings という数字は
   golden 12 ケースが増えた今も更新していない。

### 2026-08-11（6 本目）— Phase 4 に着手: `//nolint` / nolintlint / errcheck の名前

**やったこと**

Phase 3 の台帳が 99.3% で頭打ちになったので、5 本目の「次にやること 7」に従って
**「発火したか」ではなく「何を比較しているか」**の側に移った。Phase 4 の最初の 5 ケース
（`nolint` / `nolint-strict` / `nolint-allow-unused` / `errcheck` / `errcheck-verbose`）を
golden tier に載せた。**ハーネスは 1 行も変えていない** —— ケースは `config.yml` を各自持ち
`sources.txt` が同じ fixture を指すので、「fixture 1 個 × config N 個」は既存の仕組みで書ける。

#### 1. `//nolint` は 5 つの規則が違っていた（19 件中 6 件しか一致しなかった）

上流は `pkg/result/processors/nolint_filter.go`（抑止）と
`pkg/golinters/nolintlint/internal/nolintlint.go`（ディレクティブ自身の診断）の 2 本立てで、
**同じコメントを別々の正規表現で 2 回パースする**。guff は前者だけを持ち、しかも近似していた。

| # | guff | 上流 |
|---|------|------|
| 1 | 直前行の展開に**列を見ない**（「mid-expression では列がずれるから」とコメントに書いてあった） | `rangeExpander.Visit` は `nodeStartPos.Column == r.col` を要求する。列が違えば展開しない |
| 2 | 「`package` より前の `//nolint` はファイル全体」という**独自の規則** | そんな規則は無い。`ast.Walk` が最初に見る `*ast.File` の `Pos()` が `package` キーワードなので、**直上・同列**なら他のノードと同じ規則で末尾まで伸びるだけ。**空行 1 つ挟むと効かない** |
| 3 | 展開対象を `node_span` の 22 種に限定 | 全ノード（`ast.Walk`）。`node_pos`/`node_end` に委ねれば同じになる |
| 4 | `//nolint:printf` が **analyzer 名**でも一致 | `doesMatch` は `issue.FromLinter` だけを見る。`printf` は未知の名前で、**何も抑止しないし unused も報告されない**（enabled でないので落ちる） |
| 5 | `//` と空白を `trim_start()` で剥がす | `strings.TrimLeft(text, "/ ")` —— **タブは剥がさない**（`//\tnolint` はディレクティブではない） |

2 は「独自の規則を足した」のではなく、**上流の一般規則を特殊ケースとして書き写して条件を緩めた**形。
`gap/gap.go`（空行を 1 つ挟んだ `//nolint:errcheck`）が guff だけ黙る、という差分で出た。

#### 2. nolintlint は 5 種のうち 1 種しか実装されていなかった

`NewLinter` は `needs |= NeedsMachineOnly` を**無条件で**立てる。つまり
**設定を一切書かなくても** `// nolint`（先頭に空白）と `//nolint :x`（コロン前に空白）は
報告される。guff が持っていたのは unused だけで、残り 4 種は `settings.rs` に
`// DEFERRED: allow-leading-space / require-explanation / require-specific` と書かれていた。

`crates/guff-lint/src/nolintlint.rs` を新設して上流の 4 正規表現ごと移植した。
**フィルタと同じパースを使い回さないこと**が要点で、両者は実入力で食い違う:

| 入力 | フィルタ | nolintlint |
|---|---|---|
| `//nolint :errcheck` | `^nolint( \|:\|$)` に一致し、`nolint:` で始まらないので**全 linter を抑止** | `fullDirectivePattern` に一致せず**malformed**（unused 候補は出さない） |
| `//nolint:ErrCheck` | 小文字化 + エイリアス解決で `errcheck` | **`ErrCheck` のまま** |
| `//nolint:a b` | 未知の linter 名 `a b` 1 個 | malformed |

3 行目の帰結が効く: unused 候補は nolintlint 側の**生の名前**で作られ、フィルタは
`enabledLinters[生の名前]` を引くので、**`//nolint:ErrCheck` の unused は永久に報告されない**
（`ErrCheck` という名前の linter は有効化されていない）。エイリアス `//nolint:megacheck` も同じ。
guff は正規化した名前で報告していたので、**上流が黙る所で喋っていた**。
`unused/unused.go` はこの 3 形を撃ち分けるためだけの fixture。

ついでに、range 側の `matched` マップを**ディレクティブの位置キー**に移した。
展開レンジは clone なので「clone に付いた印が原本に見えない」という問題があり、
`used_keys` という別の照合表で埋め合わせていた。位置を同一性にすれば
上流の「**最初に一致したレンジだけ**が credit を得る」（`shouldPassIssue` は return する）も
そのまま書ける。

#### 3. errcheck のメッセージ —— §5 の 1 番目を潰した

`nolint` ケースを載せた時点で残った差分 8 件は全部これだった。
guff は `Error return value of \`example.com/pkg.mkerr\` is not checked`、
上流は `Error return value is not checked`。**§5 の台帳 #1 そのもの**で、
`normalize.py` が両側を同じキーに畳んでいたため、OSS でも isolate でも見えなかった。

上流の規則（`pkg/golinters/errcheck/errcheck.go` + kisielk `errcheck@v1.10.0`）:

- `selectorAndFunc` が**セレクタでない**呼び出し（`f()`、ローカル関数、func 型の変数）に
  `false` を返す → `FuncName == ""` → **名前なしの短い形**
- そうでなければ `cmp.Or(SelectorName, FuncName)`。`SelectorName` は
  `getSelectorName` の**書かれたとおりの綴り**（`w.Emit` / `os.Stdout.Write`）で、
  レシーバが識別子の連鎖でないとき（`newWriter().Emit()`、`(&w).Flush()`）は空になり、
  そこで初めて `FuncName`（= `types.Func.FullName()`）に落ちる
- `errcheck.verbose: true` なら常に `FuncName`

guff は「常に `FullName`、ただし `os.Stdout`/`os.Stderr` だけ特別扱い」という近似だった。
その特別扱いは `getSelectorName` を実装すれば**自然に出る**ので消した。
`verbose` 設定も無かったので足した（`// DEFERRED (R4 follow-up): verbose.` と書いてあった）。

#### 4. 型検査器のバグ: 完全明示のジェネリック呼び出しがパッケージを ill-typed にしていた

`errcheck` ケースの fixture に `generic[int]()` を 1 行入れたら、**guff だけ何も報告しなかった**。
追うと errcheck ではなく `guff-types` で、

```go
func one[T any]() error { return nil }
_ = one[int]()   // guff: cannot infer type arguments in call
```

`one[T any]() T` なら通り、`one[T any]() error` だけ落ちる。原因は**型アリーナの hash-cons**:

1. `func() error` が `new_signature_type` で intern される（キーの `tparams` は空）
2. `signature_set_type_params` が**その場で** `tparams` を書き換える
   → **intern 表は「空 tparams」のキーのまま、ジェネリックになった型を指している**
3. `one[int]` の実体化は `params`/`results` が変わらないので**同じ形**を alloc する
   → intern がヒットして**ジェネリックな原本の TypeId が返る**
4. 呼び出し側はまだ型引数が要ると判断し、引数 0 個から推論して失敗

`TypeArena::remutate` を足し、intern 済みの型を書き換えるときは**キーを張り替える**ようにした。
`params`/`results` に型パラメータが現れる普通のジェネリック関数は「形が変わる」ので当たらない。
**引数も返り値も型パラメータを含まない**ジェネリック関数だけが踏む。

実害は finding 1 件では済まない。**ill-typed はパッケージ単位のスイッチ**なので、
この形を含むパッケージは analyzer が丸ごとスキップされていた。**HEAD のバイナリと
同じハーネスで背中合わせに測った**ところ、ill-typed が

| target | HEAD | 修正後 |
|---|---:|---:|
| helm | 2 | **1** |
| consul | 8 | **7** |
| grafana | 29 | **26** |
| prometheus（regress） | 8 | 8 |

（baseline は上限なのでゲートは緑のまま。prometheus は動かない ——
つまりこのリポにはこの形が無い。）Phase 1 のゲートが数えているのはまさにこの数字で、
**減った分だけ黙って失われていた解析が戻った**ことになる。

**測り方の注意**: 最初この差を `GUFF_DEBUG_ILL_TYPED=1` の直接実行で測って
「8 → 0」という値を得たが、**これはキャッシュのせい**だった —— 2 回目の実行は
キャッシュに当たって型検査ごと飛ばすので ill_typed を 1 行も出さない。
`--no-cache` を付けて測り直すと両方 8 で、上の表は compat ハーネス経由で
（毎回クリーンなキャッシュで）取り直したもの。**ill-typed を数えるときは
キャッシュを切ること。**

#### 5. perf: 測れたのは +0.03s、それ以上は切り分けられなかった

regress の `full` が最初 2.720s（限界 2.510s）で赤になったので、5 本目の手順どおり
**HEAD のバイナリと交互に 3 往復**した:

| | 1 | 2 | 3 |
|---|---:|---:|---:|
| HEAD | 2.400 | 2.430 | 2.470 |
| 修正後 | 2.440 | 2.460 | 2.490 |

**3/3 とも +0.03s**（+1.3%）。ノイズ帯（5 本目の実測で 0.00〜0.02s）よりわずかに上なので
本物と見て、足した仕事を 2 つ削った ——
nolintlint が無効なら**コメントの 2 度目の解析をしない**（prometheus の config は
nolintlint を有効にしていない）、`remutate` は**新しいキーで intern し直さない**。
どちらも入れた後で測り直しても **+0.03s は動かなかった**。

そこから先は**この機械では切り分けられない**。型検査の変更だけのバイナリを足して
3 分割で測ろうとしたところで裾に入り、**同じバイナリの分散（±0.1s）が
追っている差（0.03s）を上回った**。静かなときの実測は 2.40〜2.49s で
限界 2.510s の内側、findings は 20/20 で P=R=1.0。

**「+0.03s の出どころは未特定」**として残す。次に触る人へ: 候補は
`expand_ranges` が全ノードに `node_pos`/`node_end` を計算するようになったこと
（対象は `nolint` を含むファイルのみ、prometheus では 32 ファイル）、
`suppress` がキーに `String` を clone すること、`remutate` が
ジェネリックなシグネチャ 1 本につき `InternKey` を 1 個作ること。
**測るなら profile を取ること** —— この裾の中で A/B を重ねても答えは出ない。

**結果**

- golden **17 ケース**（+5）。`nolint` 23/23、`nolint-strict` 36/36、
  `nolint-allow-unused` 15/15、`errcheck` 11/11 —— いずれも**正規化なし・allowlist なし・完全一致**。
  `errcheck-verbose` だけ ratchet 1/1（下記）。
- `cargo test --workspace` **3,024 件緑**（+9）。errcheck の単体テストは
  `contains("Error return value")` から**メッセージ完全一致**に変えた。
- isolate 115 target（`swaggo` はこのホストに `swag` が無いのでスキップ）、
  OSS pr / nightly とも据え置き（consul の allowlist 3 件のみ）。

**恒久差分 1 件（`errcheck-verbose` の ratchet 1/1）**

インターフェースのメソッドで `FullName()` がレシーバを落とす
（上流 `(pkg.emitter).Emit` / guff `pkg.Emit`）。`guff-types` が
**インターフェースのメソッドにレシーバを繋いでいない**ためで、
`subst.rs` が自ら「chunk-2 already deferred receiver wiring for interface methods」と書いている。
`errcheck` の `build_exclude_set` が `(interface).Method` / `pkg.Method` という
**別名を足して回っている**のもこれの回避策で、直せば両方消える。§7 に記録した。

**次にやること**

1. Phase 4 の続き。次は **`linters.exclusions.{rules,presets,generated,paths}` と
   `issues.exclude-rules`** —— `nolint` ケースの fixture がそのまま使える
   （既に findings が 5 linter 分ある）。その次が `severity.rules` /
   `max-issues-per-linter` / `max-same-issues` / `uniq-by-line`。
2. **§5 の台帳の残り 6 件**（unused の prefix、staticcheck のコード剥がし・言い回し・末尾ピリオド、
   modernize の prefix）。#1 と同じで、**golden ケースを 1 つ作れば正体が出る**。
   errcheck がそうだったように、`normalize.py` が畳んでいる差分は
   「表記ゆれ」ではなく**実装の食い違い**であることがある。
3. 型検査器: **インターフェースのメソッドにレシーバを繋ぐ**（上の恒久差分）。
   `remutate` と同じで、直すと複数の場所の回避策が同時に消える。
4. 5 本目からの持ち越し: gosec の DEFERRED、SA9008 の IR 検証 / SA5011 の σ、
   govet の未実装 16 pass、guff が typecheck エラーを finding として出さない件。

### 2026-08-11（7 本目）— `linters.exclusions` の 4 軸と `generated` の 4 モードを golden に載せた

**やったこと**

6 本目の「次にやること 1」に従い、Phase 4 の残りのうち
**`linters.exclusions.{rules,paths,presets,generated}`** をゴールデンゲートに載せた。
新規 8 ケース（`exclusions` / `-rules` / `-paths` / `-presets` と
`generated` / `-lax` / `-strict` / `-disable`）。ハーネスの変更は今回も 0 行。

fixture は 2 本の新規ツリー
（`crates/guff-lint/tests/testdata/exclusionsem/`、`crates/guff-lint/tests/testdata/gensem/`）。
exclusions 側は **baseline ケースを 1 つ置き、他の 3 つは config キーを 1 個だけ足した同じ config**
にしてある。したがって**ゴールデン同士の差分がその設定の効果そのもの**になり、
presets ケースの差分は `processors.LinterExclusionPresets` の列挙そのものになる。

**73 findings 中 62 しか一致せず、残り 11 は全部実バグだった。**

| 種別 | 件数 | 内容 |
|------|-----:|------|
| 過剰除外 | 1 | 除外規則の `linters:` が **analyzer 名**と別名正規化形にも一致していた |
| 過剰除外 | 4 | `text:` / `source:` を全部 `(?i)` 付きでコンパイルしていた |
| severity | 4 | **revive の findings が severity 空**だった（baseline ケースでの件数。revive を回す全ケースに同じ欠陥がある） |
| recall | 1 | `linters.exclusions.generated` の既定を `lax` にしていた（上流は **strict**） |
| precision | 1 | lax 判定が**パッケージ節より前**のコメントしか読んでいなかった |

#### 1. `linters:` は linter 名を逐語で見る。analyzer 名ではない

上流は `baseRule.matchLinter` = `slices.Contains(r.linters, issue.FromLinter)`。
guff は `from_linter` と **`analyzer`** の両方を、しかも `normalize_linter_name` を
通した形でも照合していた。`linters: [printf]` は上流では**何にも一致しない**
（`printf` という linter は存在しない。メッセージ本文にどれだけ大きく `printf: ` と
出ていても関係ない）のに、guff は govet の findings を消していた。
**6 本目に `//nolint:printf` で見つけたのと同じ形の間違いが、除外規則側にも居た**
（`doesMatch` も `issue.FromLinter` しか見ない）。

#### 2. v2 の除外規則は大文字小文字を区別する

上流 v2 は `parseRules(excludeRules, "", newExcludeRule)` ——
**プレフィックス空文字**でコンパイルする。`issues.exclude-case-sensitive` は v1 のキーで、
v2 の `issues` セクションは `max-issues-per-linter` / `max-same-issues` / `uniq-by-line` /
`new-from-*` / `whole-files` / `fix` **だけ**しか持たない。severity 規則も同じく空プレフィックス。
guff は serde の既定 `false` からずっと `(?i)` を足しており、
`text: ERROR RETURN VALUE` が errcheck の findings を全部消していた。
`(?i)` が要る唯一の規則（EXC0001）は**上流がパターン自身に書いている**。

`Config::effective_severity()` を足して、v2 では `case_sensitive: true` として読むようにした
（`effective_issues()` と同じ形）。

#### 3. preset 表が v1 のままだった

guff の v2 preset 表は `EXC0001`…`EXC0015` の **v1 の 15 件**で、上流 v2 の
`LinterExclusionPresets` は **13 件**。差は EXC0002 / EXC0003（`golint` —— v2 に存在しない）と、
EXC0011 の linter が `stylecheck` ではなく **`staticcheck`** であること。
linter 名を逐語で比べる以上、**v1 の残骸は「何もしない」か「過剰除外」にしかならない**。
EXC0011 が今まで効いていたのは 1 の正規化のおかげで、つまり
**別の場所で findings を消していたのと同じ緩さに助けられていた**。表を上流と 1 件ずつ揃え、
`InternalReference` の ID も対照用に持たせた。

#### 4. revive の severity は空にならない

上流 `severity(cfg, failure)` は「そのルールの実効 severity がちょうど `error` なら error、
それ以外は **warning**」。`normalizeConfig` が `revive.severity` を `warning` で既定化し、
自前の severity を持たない全ルールにそれを配るので、**空になる経路が無い**。
guff は「設定されていなければ空」だった。

**既存の `cases/revive` はこれを見られない** —— あのケースは `severity: warning` を
明示しているので、空との差が出ない。設定を書かない config を golden に載せて初めて出た。
gosec の severity（5 本目）と同じ「このティアしか比較しないフィールド」の 2 例目。

#### 5. `generated` の既定は `lax` ではなく `strict`

これが今回いちばん効く 1 件。`GeneratedModeLax` という定数があり、matcher は
`mode == ""` を lax として扱う。だから processor だけ読むと lax に見える。
しかし **`config.Loader.Load` が空値を `GeneratedModeStrict` に書き換えてから** processor に渡す:

```go
if l.cfg.Linters.Exclusions.Generated == "" {
    l.cfg.Linters.Exclusions.Generated = GeneratedModeStrict
}
```

書き換えられるのは `linters` 側だけで、`formatters.exclusions.generated` は空のまま
= lax。つまり **`golangci-lint run` と `golangci-lint fmt` で「生成ファイル」の定義が違い、
どちらも上流の仕様**。guff は両方 lax にしていたので、
**先頭コメントに "do not edit" や "autogenerated file" が地の文で入っているだけのファイル**の
findings を黙って捨てていた。`run` 側を strict に直した（formatter 側は lax のままが正しい）。

**「既定値は挙動であり、型に書いてあるとは限らない」** —— 定数と matcher を読んで
lax と判断するのが自然な形になっている。既定を**書かない**ケース（`cases/generated`）が
これを捕まえた。設定を明示するケースだけ作っていたら永久に見つからない。

#### 6. lax はパッケージ節の**下**のコメントも読む

上流は `parser.ParseFile(…, PackageClauseOnly|ParseComments)` の
**collect した全コメントグループ**を連結する。`PackageClauseOnly` は
「パッケージ節で読むのをやめる」ではない: パーサは 1 トークン先まで読むので、
`package p` の**直下のグループ**も（空行で分かれた複数グループでも）入る。
止まるのは節の後の最初の非コメントトークン（`import` など）。

推測せず go/parser に実際に食わせて確かめた（`getComments` をそのまま写した probe）。
guff の `leading_comment_doc` はパッケージ節で break していたので、
`generated-lax` の `after/after.go` を上流だけが生成ファイルと見なす差分になった。
ついでに `ast.CommentGroup.Text` が落とす**ディレクティブ**（`//name:value`、`//line ` 等）も
落とすようにした —— `// go:...` のように**空白があれば地の文**、という非対称も含めて写した。

**恒久差分でも ratchet でもなく、8 ケースすべて 0/0 で緑。**

**fixture が 1 件だけ埋められない**: EXC0010（gosec **G304**）。
guff は G304 を実装していない（`gosec.rs` の DEFERRED）ので、fixture を置くと
比較ではなく永久 missing になる。**ルールを実装するときに fixture も足すこと。**

**ゲートに載せられない差分（記録のみ）**

上流は**条件が 2 個未満の除外規則を設定エラーとして拒否する**
（`BaseRule.Validate(excludeRuleMinConditionsCount=2)`。severity 規則は 1 個）。
`- linters: [errcheck]` だけの規則は「広い規則」ではなく**config エラー**で、
golangci-lint はそもそも起動しない。guff はこれを受け入れて errcheck を全部消す。
`path` と `path-except` の同時指定も上流はエラー。preset 名の検証も同様
（guff は `stdErrorHandling` のような camelCase も受ける）。
**golden tier は「上流が起動を拒む」を表現できない**ので、実装するなら
`config.rs` に validate を足して `ConfigError` を返す形になる。次にやること 2。

**結果**

- golden **25 ケース**（+8）。新規 8 ケースはすべて**正規化なし・allowlist なし・ratchet なし**で完全一致。
  既存 17 ケースは据え置き（staticcheck / revive の ratchet も動かず）。
- `cargo test --workspace` **3,025 件緑**。`config_test` の
  「v2 の generated 既定は lax」というアサーションは**テストの方が間違っていた**ので直した。
- isolate 115 target OK（`swaggo` はこのホストに `swag` が無く実行不能。6 本目と同じ）。
- OSS pr / nightly とも据え置き（consul の allowlist 3 件のみ、他は P=R=100%）。
  **`generated` の既定変更は OSS の全ターゲット（gin / caddy / helm / consul / grafana /
  containerd）の findings を 1 件も動かさなかった** —— 実 config は生成ファイルを
  `paths` で先に落としているか、lax と strict の差になるファイルを持っていない。
- regress full: findings 20/20 P=R=1.0 で不変。**wall 2.430s（限界 2.510s）で PASS**。

**perf の測り方（今回ハマった所）**

最初の 3 回は 2.800 / 2.740 / 2.740s で赤だった。原因は**ホストの混雑**で、
コードではない。切り分けは `--skip-golangci` で guff だけを 3 連続測る形が速くて確実だった
（golangci-lint のパスは guff の計測の**後**に走るので、このフラグは計測前の条件を何も変えない）:

| | 1 | 2 | 3 |
|---|---:|---:|---:|
| `--skip-golangci` | 2.480 | 2.440 | 2.370 |

**3 回とも限界の内側**で、しかも回を追うごとに速くなる（機械が冷えていく）形。
その後にフル profile を回して 2.430s / PASS。

なお 5 本目・6 本目が使っていた「HEAD のバイナリと交互に測る」は今回**やっていない**。
prometheus の config が踏むのは
(a) 除外規則の linter 照合（**正規化を 1 段減らしたので仕事は減る**）、
(b) `generated` の既定 strict 化（lax より**軽い**走査）、
(c) revive の severity（アロケーション数は変わらない）で、
**遅くなる機序が無い**うえ、上の 3 連続が限界の内側に収まったため。
このホストは常時 load 1.0 前後の何かが走っている状態が続くことがあるので、
**赤が出たらまず `--skip-golangci` を 3 回**、それでも赤なら A/B に進むこと。

**次にやること**

1. Phase 4 の続き。残りは `issues.uniq-by-line` / `max-issues-per-linter` /
   `max-same-issues` / `severity.rules` と、`run.build-tags` / `run.tests` / `run.go`。
   **`severity.rules` は `exclusions` の fixture がそのまま使える**（6 linter 分の findings が既にある）。
   `max-*` は「どの順で切るか」がそのまま出るので、`apply` の並べ替え（linter 名順）を検証できる。
2. **config の validate**（上記「ゲートに載せられない差分」）。上流が拒む config を guff も拒む。
   実装先は `config.rs`（`ConfigError` を返す）。**着手前の調査は済んでいる**:
   `corpus/cache/*/.golangci.y*ml` と `tests/testdata/config_corpus/*.yml` の
   **68 config を走査して、条件 1 個の除外規則は 0 件**だった（上流がそれらを
   実行できている以上そうなるはずで、実測でも裏が取れた）。したがってこの validate を
   足しても OSS / regress のゲートは動かない。
3. **gosec G304 の実装**（`readfile.go` の移植）。`filepath.Clean` / `Join` の追跡と
   `TryResolve` が要る。実装したら `exclusionsem/presets` に fixture を戻し、
   EXC0010 が preset ケースで比較されるようにする。
4. 6 本目からの持ち越し: §5 の台帳の残り 6 件、インターフェースメソッドのレシーバ配線、
   gosec の他の DEFERRED、SA9008 / SA5011 の σ、govet の未実装 16 pass。

---

### 2026-08-12（8 本目）— Phase 4 のランナー側を閉じた: `issues.max-*` / `severity` / `run.*` / `staticcheck.checks`

**やったこと**

7 本目の「次にやること 1」に従い、Phase 4 の残りのうち**ランナー側の設定をすべて**
ゴールデンゲートに載せ、続けて linter settings の 1 本目（staticcheck の `checks`）も載せた。
新規 18 ケース:

| グループ | ケース | 測っているもの |
|---|---|---|
| issues（重複） | `issues-uniq-by-line`（baseline は `exclusions`） | `issues.uniq-by-line` |
| issues（上限） | `issues-limits` / `-max-per-linter` / `-max-same` / `-max-both` | `max-issues-per-linter` / `max-same-issues` |
| severity | `severity-default` / `-rules` / `-linter`（baseline は `exclusions`） | `severity.default` / `severity.rules` |
| run | `run-tests` / `-off`、`run-build-tags` / `-none`、`run-go` / `-122` | `run.tests` / `run.build-tags` / `run.go` |
| staticcheck | `staticcheck-checks-{default,all,glob,not-s}` | `linters.settings.staticcheck.checks` |

**142 findings 中 100 しか一致せず、残り 42 は全部実バグだった。**

| 種別 | 件数 | 内容 |
|------|-----:|------|
| severity | 33 | **v2 の `severity.default` を一度も読んでいなかった**（キー名が v1 と違う） |
| precision | 3 | `run.go` が gofumpt の `-lang` にしか配線されていなかった |
| recall | 4 | `-S*` を接頭辞照合していた（SA / ST まで消していた） |
| recall | 3 | `checks: ["S*"]` で有効なチェックが 0 になり `no linters enabled` で落ちた（ケース丸ごと） |
| recall | 1 | `max-*` 2 つの**適用順が上流と逆**だった |
| recall | 1 | SA9003 を既定 disabled に入れていた |

#### 0. ハーネスに初めて手を入れた —— 測れなかったのはフラグのせい

`run.sh` は golangci-lint に `--max-issues-per-linter=0 --max-same-issues=0` を
渡していた。既定の 50 / 3 で golden が黙って切られるのを防ぐためだが、
**CLI フラグは config に勝つ**ので、この 2 キーは golden tier では**測りようがなかった**。

フラグを外し、代わりに**各ケースの config が 2 キーを必ず書くことを `run.sh` が要求する**
形にした（測る対象でないケースは `0`）。既存 25 ケースは**全部すでに書いていた**ので
golden は 1 バイトも動かない（`exclusions` を再生成して差分 0 を確認済み）。
これで「ケースの設定は config.yml がすべて」が本当になり、黙った切り捨ても防げる。

#### 1. `severity.default` は v1 と v2 でキー名が違う

v1 は `severity.default-severity`、**v2 は `severity.default`**。guff は v1 の名前しか
読んでおらず、serde は知らないキーを黙って捨てるので、**v2 config の `severity` セクションは
まるごと no-op** だった。`severity-default` の 24 findings は全部リンタが付けた等級のまま、
`severity-rules` では**規則自体は正しく当たっているのに**既定だけが落ちて 9 件ずれた。

`SeverityConfig` に v2 用の `default` フィールドを足し、`effective_severity()` が
バージョンごとに**自分の綴りだけ**を読むようにした（v2 config に `default-severity` と
書いても上流はファイルごと拒否するので、そこを寛容にするのは 7 本目に潰したのと同じ形の間違い）。

上流の意味論はこの 3 行に尽きる（`processors/severity.go`）:

- `default` は**未設定の finding に足す**ものではなく、**全部に上書きする**。
  gosec の `low` / `medium` も revive の `warning` も `error` になる
- `@linter` は severity ではなく**番兵**で、値を書かずに `return` する
- `Validate` により **rules があるなら default は必須**、各 rule の severity も必須、
  条件は 1 個で足りる（除外規則は 2 個必要 —— この非対称は上流の
  `severityRuleMinConditionsCount = 1` / `excludeRuleMinConditionsCount = 2` そのもの）

`severity-linter`（`default: "@linter"`）のゴールデンは `exclusions` と**バイト一致**する。
「何も起きない」ことをケースにしたのは、**そこが静かに壊れる**からで、
`@linter` をただの文字列として扱えば全 finding に `@linter` と書かれ、
「default なし」と解釈して代入だけ走らせれば等級が空になる。

#### 2. `run.go` はソースの性質ではなく**リンタに渡す値**

`Loader.handleGoVersion` は `run.go` を `Settings.Govet.Go` /
`Settings.Revive.Go` / `Settings.Gocritic.Go` / gofumpt の `-lang` /
`GOSECGOVERSION` に**コピーする**。したがって:

- `govet` は **`loopclosure` をアナライザ集合から外す**（黙らせるのではない）
- revive の `range-val-in-closure` / `range-val-address` は
  その版を `IsAtLeastGoVersion` で読む

`go 1.21` のモジュールを `run.go: "1.22"` で lint すると、
**ツールチェーンは 1.21 としてコンパイルし続けるのに findings は 3 件消える**。
guff は `run.go` を gofumpt にしか渡しておらず、他はすべてモジュールの go directive を
見ていたので 3 件とも撃っていた。

直したのは 3 箇所（上流の `handleGoVersion` に対応する 1 メソッドに集約した）:
`LinterSettings::apply_go_version` が govet / revive / gocritic の settings に版を書き、
`filter_govet` が 1.22 以上なら `loopclosure` を外し、
`guff_revive::util::go_version_at_least` と gocritic の版取得が
**設定値 → モジュール**の順に読む。未設定なら上流も「検出した版＝モジュールの版」を
入れるだけなので、フォールバックが同じ答えになる。

**`run.go` はキャッシュキーにも入れた。** `settings_fingerprint` は
`linters.settings` の生 YAML なので、`run.go` だけが違う 2 回の実行を区別できない。
これは golden tier（`--no-cache`）では見えない種類のバグで、直すのは 1 行だが
**入れ忘れると「2 回目だけ結果が違う」**という最悪の壊れ方をする。

#### 3. `max-*` は 2 つ揃って初めて順序が見える

上流の順は `UniqByLine → MaxPerFileFromLinter → MaxSameIssues → MaxFromLinter`。
guff は per-linter を先にやっていた。errcheck が同じ文言を 3 件、別の文言を 2 件出す
fixture に `max-issues-per-linter: 3` + `max-same-issues: 1` を与えると:

- 上流: 文言で切って 2 件（1 件目と 4 件目）→ per-linter 3 に収まるのでそのまま **2 件**
- guff（旧）: per-linter で先頭 3 件（全部同じ文言）→ 文言で切って **1 件**

`issues-max-per-linter` と `issues-max-same` は**どちらも一発で一致した**。
バグは `issues-max-both` にしか出ない。**片方ずつのケースだけ書いていたら見つからない。**

`MaxPerFileFromLinter`（フォーマッタ系 6 つを 1 ファイル 1 件に制限）は guff に
実装が無いが差は出ない —— guff の formatter は**もともとファイルごとに 1 件**しか出さない
（`check_files_multi` に実測つきのコメントがある）。`issues.fix: true` のときだけ
上流が hunk ごとに出す側に回るが、それは fixer の話でこのゲートでは表現できない。

#### 4. 上流の順序が再現しない場所には fixture を置かない

`max-issues-per-linter: 2` を `exclusions` fixture（revive 入り）に当てると、
**3 回連続で別々の revive finding が残った**。revive はパッケージ内のファイルを並行に
lint するので、どの finding が先に届くかがレース。上限系は「先頭 N を残す」処理なので、
**到着順が再現しない linter を混ぜた瞬間にゴールデンが揺れる**。

そこで上限系の fixture は**1 パッケージ 1 ファイル**にした（到着順 = AST 走査順）。
逆に `uniq-by-line` は揺れない: `Runner.Run` が linter 名順に issues を追加するので、
同じ行を複数の linter が撃ったときの勝者は決まっている。
`issues-uniq-by-line` のゴールデンはその順序ごと固定している
（errcheck > gosec > govet、revive > staticcheck ＝ 名前順）。
guff は 5 本目までにこの並べ替えを入れてあったので、このケースは一発で一致した。

#### 5. `output.path-mode` はこのゲートに載せられない —— 代わりに実測した

`run.sh` は golangci-lint に `--path-mode abs` を渡し、`golden.py` がモジュール相対に
正規化する。**それはこの設定が変えるものと同じ正規化**なので、ゲートの中では原理的に
比較できない。手で実測した結果（`compat/golden/.work/exclusions`、両ツール）:

| `output.path-mode` | golangci-lint | guff |
|---|---|---|
| 未設定 | **config ファイルのディレクトリ基準**の相対パス（`../../../../…`） | **cwd 基準** |
| `abs` | 絶対パス | 絶対パス（一致） |
| `rel` | **config error**（`validatePathMode` は `""` と `abs` のみ） | 受理して相対扱い |

1 行目が食い違うのは**config が lint 対象ディレクトリの外にあるとき**だけで、
それはこのハーネスの事情であってユーザの通常運用ではない（リポジトリ直下に
`.golangci.yml` を置けば cfg 基準 = cwd）。2 行目は「上流が拒む config を guff も拒む」
（7 本目の次にやること 2）と同じ箱に入る。**どちらも直していない。ここに記録した。**

#### 6. `staticcheck.checks` —— 実際のユーザが一番よく書く settings キー

ランナー側を閉じたので linter settings に着手した。1 本目に `staticcheck.checks` を選んだのは
**`corpus/` の 16 config 中 7 つが書いている**から（実測）。文法は honnef のもので、golangci-lint は
`filterAnalyzerNames` を**コメントつきで丸ごとコピー**している。

fixture は 1 ファイルに 4 カテゴリ（S / SA / ST / QF）から 1 件ずつ + SA9003 の空ブランチ。
4 ケース（既定 / `all` / `S*` / `all,-S*`）で **23 findings 中 15 一致**、
残り 8 件は 30 行の関数に入っていた 3 つのバグだった:

1. **glob は接頭辞ではなくカテゴリ**。上流は名前を**最初の数字で切って**カテゴリを比較する
   ので、`S*` は S1002 に当たり **SA1006 / ST1017 には当たらない**。
   guff は `starts_with` だったので `-S*` が SA と ST も消していた（precision 3 件）。
   逆に**数字を含む glob（`S1*`）は素の接頭辞照合**で、2 つの綴りは意味が違う。
2. **正の glob が実装されていなかった** —— `checks: ["S*"]` は「そういう名前のチェック」と
   解釈され、有効なチェックが 0 になって **guff が `no linters enabled` で落ちていた**。
   `all` を含まないリストは空集合から始まるので、正の glob だけが唯一の点火手段になる。
3. **既定リストに SA9003 を入れていた**。上流の `defaultChecks` は
   `all` から **ST を 6 つ引いただけ**で、「空ブランチ」がどれだけ opinionated に読めても
   そこには入っていない。**`checks` を書かない config（＝大半）で SA9003 が黙って消えていた**。
   既存ケースは全部 `checks: [all]` を明示していたので、これを見られるケースが 1 つも無かった。

**セレクタ列は集合 2 つではなく map**（`allowedChecks[name] = b` を順に書く）なので、
`["-ST1000", "all"]` は ST1000 を**有効にする**し `["all", "-ST1000"]` はしない。
guff の旧実装は「disabled が常に勝つ」形で、この順序を落としていた。
corpus の config はどれも `all` を先に書くので**実データでは絶対に露見しない**。

さらに、**同じ文法を 2 か所で読んでいた**（アナライザのフィルタと、
SSA の debug ref が要るかを決める `staticcheck_check_enabled`）。
2 か所は別々に壊れていたので、**名前の集合を受け取って allow-map を返す 1 関数**に寄せた。

**ゲート**

- `./compat/golden/run.sh` — **43 ケース**（新規 18）。ratchet 4 本は baseline のまま
- `cargo test --workspace` — 緑。新規単体テスト 6 本
  （`max_same_issues_runs_before_max_issues_per_linter`、
  `v2_severity_default_is_spelled_default_not_default_severity`、
  `run_go_at_least_122_drops_loopclosure`、
  `apply_go_version_reaches_every_linter_the_loader_writes`、
  `staticcheck_checks_glob_is_a_category_not_a_prefix`、
  `staticcheck_default_checks_turn_off_six_st_checks_and_nothing_else`）
- `./compat/run.sh --oss --tier pr,nightly` — 8 ターゲット緑（allowlist は consul の 3 件のまま、
  consul は guff=258 / golangci=255 / P=98.8% / R=100%）。
  **SA9003 を既定で有効にしたのに OSS の diff が増えていない**のが重要な確認で、
  上流も同じ findings を出しているから allowlist が動かない
- `./compat/run.sh --isolate` — **116 ターゲット**全部緑。
  ただし最初の実行は `swaggo` だけ落ちた。原因は**このホストに `swag` が入っていなかった**だけで、
  5 本目がピンしたもの（`go install github.com/swaggo/swag/cmd/swag@93e86851e9f2…`）を入れたら 1/1 一致。
  **エラーメッセージが紛らわしい**ので次に踏む人のために書いておく ——
  `guff: swaggo: ./bad.go: No such file or directory (os error 2)` は
  **`bad.go` ではなく `swag` バイナリが無い**という意味（`FormatError::Io` が
  `Command::new(bin)` の spawn 失敗にソースファイルのパスを添えて表示する）。
  shell out するフォーマッタ（gci / gofmt / gofumpt / golines / swaggo）は全部この形なので、
  「ファイルが無い」と言われたら**まず PATH を疑うこと**
- `compat/coverage.py observe && report` — 台帳の数字は**動かない**（543 / 1 / 3）。
  Phase 4 は「発火したか」ではなく「同じ config で同じものを撃つか」を増やす投資なので想定どおり
- `./regress/run.sh --profile full`（cold wall / prometheus `./...`）は
  **この変更では動いていない**。静かなホストで 3 回ずつ測った A/B:

  | | 1 | 2 | 3 | median |
  |---|---:|---:|---:|---:|
  | この作業ツリー | 2.480s | 2.490s | 2.520s | **2.490s** |
  | HEAD（`crates/` を stash して再ビルド） | 2.510s | 2.490s | 2.450s | **2.490s** |

  peak RSS も 3.06–3.10 GB で両者同じ（baseline 3.11 GB より下）。finding-set は
  `both=20 / guff_only=0 / golangci_only=0` で不動。
  **ゲート自体は両方とも赤**で、上限が `2.360 × 1.0 + 0.150 = 2.510s`、
  つまり**このホストの今日の素の値が上限に重なっている**。7 本目が書いた手順
  （赤 → `--skip-golangci` を 3 回 → それでも赤なら A/B）をそのまま踏んで、
  **原因は今回の変更ではない**ところまで確定させた。baseline は**動かしていない**
  —— 原因の分からない再測定で上限を緩めるのは、次の退行を見えなくするだけだから。
  最初の 1 回が 2.840s だったのは単にホストが混んでいたためで、
  `regress/run.sh` の perf-guard（load > ncpu/4 で拒否）がそれを弾くようになっている。
- OSS / regress は**影響を受けない**ことを config 側から確認済み:
  corpus の 68 config に**トップレベル `severity:` セクションを持つものは 1 つも無く**
  （`config_corpus/telegraf.yml` だけが持つが、これはパースのみの fixture）、
  `run.go` を書いているものも無く、`max-*` を書いているものは全部 `0`。
  したがって今回の 3 つの修正はどれも既存ゲートの入力を変えない

**次にやること**

1. **Phase 4 の残りは linter ごとの settings キーだけ**になった。ランナー側（issues /
   severity / run / exclusions / nolint）は閉じ、linter settings は errcheck の `verbose` と
   staticcheck の `checks` の 2 つが済んでいる。次は 1 セッション 2〜3 linter で、
   **finding-set を変えるキーから**やる: `gosec.{severity,confidence,includes,excludes}`、
   `revive.{confidence,severity,enable-all-rules}`、`govet.{enable-all,disable-all}`、
   `gocritic.{enabled-tags,disabled-checks}`、`errcheck.{check-blank,check-type-assertions,exclude-functions}`。
   `exclusionsem` / `issuesem` / `staticchecksem` の fixture がそのまま使える。
   **狙い目は「既定値」と「セレクタ文法」**で、8 本目の 3 バグはどちらもその 2 種類だった
   —— 既存ケースがキーを明示していると既定は永久に測られない。
2. **config の validate**（7 本目からの持ち越し、優先度は上がった）。今回さらに 2 件見つかった:
   `severity.rules` があるのに `severity.default` が無い config と、
   `output.path-mode: rel`。上流はどちらも起動を拒む。実装先は `config.rs`（`ConfigError`）。
   **「上流が拒む config を guff は受理する」は golden tier で表現できない**ので、
   ここに列挙が溜まっていく形になる —— 実装するときは §4 のこの節と 7 本目の同項を突き合わせること。
3. **gosec G304 の実装**（7 本目からの持ち越し）。`readfile.go` の移植。実装したら
   `exclusionsem/presets` に fixture を戻し、EXC0010 が preset ケースで比較されるようにする。
4. 6 本目からの持ち越し: §5 の台帳の残り 6 件、インターフェースメソッドのレシーバ配線、
   gosec の他の DEFERRED、SA9008 / SA5011 の σ、govet の未実装 16 pass。

### 2026-08-12（9 本目）— linter ごとの settings キー 5 本: errcheck / govet / gocritic / revive / gosec

**やったこと**

8 本目の「次にやること 1」に従い、Phase 4 の唯一の残件だった **linter ごとの settings キー**を
5 linter ぶんゴールデンゲートに載せた。新規 34 ケースで **43 → 77 ケース**:

| グループ | ケース | 測っているもの |
|---|---|---|
| errcheck | `errcheck-opts`（baseline）/ `-blank` / `-asserts` / `-exclude-functions` / `-no-default-exclusions` | `check-blank` / `check-type-assertions` / `exclude-functions` / `disable-default-exclusions` |
| govet | `govet-settings`（baseline）/ `-disable` / `-enable-wins` / `-disable-all` / `-enable-all` | `enable` / `disable` / `enable-all` / `disable-all` と**既定のアナライザ集合** |
| gocritic | `gocritic-settings`（baseline）/ `-enabled-tags` / `-enabled-checks` / `-disabled-checks` / `-disabled-tags` / `-disable-all` / `-enable-all` | 6 つのセレクタキーと**既定のチェッカ集合** |
| revive | `revive-settings`（baseline）/ `-confidence-{085,095,0}` / `-severity-{error,info,rule}` / `-enable-{default,all}-rules` | `confidence` / `severity` / `enable-*-rules` |
| gosec | `gosec-settings`（baseline）/ `-default-rules` / `-severity-{medium,high}` / `-confidence-{medium,high}` / `-includes` / `-excludes` | `severity` / `confidence` / `includes` / `excludes` と**既定のルール集合** |

**104 findings 中 97 しか一致せず、差分は 4 か所**だった。ただし**バグは 10 個**で、
差分が指した 4 か所のうち 1 つ（errcheck の列）が、上流の 80 行を読んだ結果
**同じ関数の中の別の 3 バグ**に化け、`enabled-tags` を直したことで初めて突き合わせられた
k9s がさらに 1 つ（`boolExprSimplify`）を出した。8 本目までと同じ形（ゴールデン差分は
「どこが」しか言わない）だが、今回は**新しい fixture を書かないと残りは永久に測れない**
ところまで行った。

| linter | 差分 | 実バグ | 内容 |
|---|---:|---:|---|
| errcheck | 3 | 4 | 型アサーションの位置と、上流の**枝刈り**、それに括弧 |
| govet | 1 | 2 | `enable` と `disable` の優先順、既定集合 |
| gocritic | 4 | 4 | `enabled-tags` が**フィルタになっていた**、`disabled-tags` の適用順、タグ表、`boolExprSimplify` の untyped bool |
| revive | 0 | 0 | 9 ケースすべて一発一致 |
| gosec | 0 | 0 | 8 ケースすべて一発一致 |

#### 1. errcheck —— ゴールデンが指したのは列で、直したのは走査そのもの

`errcheck-asserts` の 9 件中 3 件が**列だけ**ずれていた。`_ = i.(string)` は合っていて
`return i.(string)` はずれる、という形で、原因は `visit_type_assert` が `.(` の位置を
報告していたこと。上流の `checkAssertExpr` は `expr.Pos()` で、`ast.TypeAssertExpr` は
**オペランドから始まる**（`i.(string)` なら `i`）。**オペランドが 1 文字のときだけ
両者が一致する**ので、既存の fixture はその 1 文字の形しか持っていなかった。

ここで `errcheck.go` の `Visit` を読むと、この関数の周りに**まだ測っていないもの**が
3 つあった。scratch モジュールに書いて両方のツールを走らせた結果:

| 形 | 上流 | guff（当時） |
|---|---|---|
| `var a = i.(string)` | 1 件 | **2 件** |
| `var b, ok = i.(string)` | 0 件 | **1 件** |
| `_ = (f())`（`check-blank`） | 0 件 | **1 件** |
| `(_) = f()` | 0 件 | **1 件** |
| `_ = (i.(string))` | 7 列目 | **6 列目** |
| `_ = (func() error { f(); return nil })().(error)` | 1 件 | **2 件** |

原因は 2 つ:

- **上流は枝刈りする。** `case *ast.TypeAssertExpr` は無条件に `return nil` を返し、
  単一 RHS がアサーションの代入も `followed=false` で `return nil` になる。
  刈られた部分木は**一切見られない**ので、その中の関数リテラルにある未チェック呼び出しも
  報告されない。guff は `preorder_typed` で全ノードを平坦に舐めていて、
  `AssignStmt` の分だけ「lparen の位置」を skip セットに入れる形で近似していた
  —— `GenDecl` は入れていなかったので `var a = i.(string)` が二重に出て、
  `var b, ok = …` に至っては**上流が出さないものを出していた**。
  ノードは親→子・兄弟は位置順に来るので、**`skip_until` という単調な水位**を置けば
  枝刈りと同じことが言える（AST のスパンは入れ子になるので、水位より前にある後続ノードは
  水位を立てた部分木の中にしかいない）。
- **上流はどこでも括弧を剥がさない。** `rhs[0].(*ast.CallExpr)` は `(f())` に当たらないので
  それは blank assignment ではなく、`(i.(string))` も同様に落ちて**アサーション自身の visit**が
  報告する ＝ 括弧の内側の列になる。guff は `check_assignment` の 6 か所で `unparen` していた。
  6 か所とも上流に対応物が無い。

ついでに、多値代入の腕は上流だと**`_` でなくても報告する**（`id.Name == "_"` の判定は
呼び出しの腕の中にしかない）ので `a, c := i.(string), j.(int)` は 4 件になる ——
2 つの名前 + 走査が続くので 2 つのアサーション自身。guff はループ先頭で `_` を要求していた。

**この 6 行は fixture が無いと二度と測れない**ので、
`crates/guff-errcheck/tests/testdata/assert_shapes/bad.go` を足して 5 ケース全部に載せ、
Rust 側にも `line:col` で突き合わせるテストを 1 本足した
（`support::run_analyzer_positions` を新設。従来の `run_analyzer` はメッセージしか返さず、
**列が動いても単体テストからは永久に見えない**）。

#### 2. govet —— `enable` は `disable` より先に読まれる

`isAnalyzerEnabled` の腕の順序がこの関数のすべてで、guff は
「レジストリ − disable」という集合演算に潰していた。したがって
**両方のリストに同じ名前を書くと上流は有効・guff は無効**になる。
`govet-enable-wins` のゴールデンは `govet-settings` と**バイト一致**する。

もう 1 つ、`default` の腕は `defaultAnalyzers`（cmd/vet の集合）であって
「全部」ではない。guff は自分のレジストリ全体を既定にしていた。
**今日の guff には差が出ない** —— 実装している 30 個は全部 `defaultAnalyzers` の中にあり、
`allAnalyzers` にしかない 10 個（nilness / shadow / fieldalignment / unusedwrite /
atomicalign / deepequalerrors / reflectvaluecompare / sortslice / httpmux / findcall）は
1 つも実装していないから。**が、その 1 つ目を移植した日に既定で発火する**ので、
`GOVET_DEFAULT_ANALYZERS` を置いて `govet_default_set_is_not_every_analyzer` で留めた。
**「まだ差が出ないバグ」は差が出ないうちにしか安く直せない。**

#### 3. gocritic —— `enabled-tags` はフィルタではなく**和集合**

`enabled-tags: [performance]` に対して **guff は 0 件、上流は 3 件**だった。
`inferEnabledChecks` は既定集合から始めてタグの付いたチェッカを**足す**。
guff は「そのタグを持つものだけ残す」と読んでいて、
**既定 ON のチェッカは定義上どれもオプトインタグを持たない**（既定集合とは
experimental / opinionated / performance / security のどれも持たないもののこと）ので、
**このキーを書いた瞬間に既定の結果が丸ごと消えていた**。

corpus でこのキーを書いているリポジトリ直下 config は **k9s だけ**（vendor 配下の
fxamacker/cbor にもあるが、それは lint 対象の config ではない）。k9s は 5 つのタグを
**まとめて**書くので消えるのは「タグ表に載っていない既定 ON のチェッカ」だが、
そもそも **k9s は `corpus/repos.json` の tier に入っていない**。
OSS ゲートがこれを緑のまま通していたのは allowlist のせいではなく、
**このキーを書く config を 1 つも走らせていなかった**から。

さらに 2 つ:

- **5 つの手順には順序がある**（base → enabled-tags → enabled-checks → disabled-tags →
  disabled-checks）。4 と 5 が 2 と 3 の後なので、
  **`enabled-checks` で名指ししたチェッカでも、そのタグが `disabled-tags` にあれば消える**。
  guff は「明示的に名指しされたものはタグフィルタの例外」にしていた ——
  そう読みたくなるが違う。
- **タグ表が 1 チェッカ 1 タグで、しかも歯抜けだった。** 和集合にした以上、
  表の欠けは「静かに足されないチェッカ」になる。go-critic v0.14.4 の
  `checkers/*_checker.go`（`info.Tags`）と `checkers/rulesdata/rulesdata.go`（`DocTags`）から
  **107 チェッカ全部**を生成し直した（`unnamedResult` のように 3 つ持つものがある）。
  同時に `DEFAULT_CHECKS` は**表からの導出**と一致することを
  `gocritic_default_checks_are_exactly_the_untagged_ones` で固定した ——
  上流に既定リストは存在せず、あるのは述語だけなので、手書きのリストは
  移植のたびに静かにずれる側にある。

#### 4. revive と gosec —— 17 ケースが一発で一致した

revive の `confidence` は**閾値を切るのが golangci-lint 側**（`wrapper.run` が
`failure.Confidence < w.conf.Confidence` で捨てる）で、fixture は
**信頼度 0.8 / 0.9 / 1 の 3 段**を 1 ルールずつ持つ（increment-decrement / error-naming /
errorf）。閾値を上げるたびに 1 件ずつ減る。`confidence: 0` が
**`revive-settings` とバイト一致する**のが今回の目玉で、`normalizeConfig` が
`cmp.Or` で既定を入れる ＝ **ゼロ値は「未設定」**だから
「0 なら全部出す」にはならない。

`severity` は 2 値スイッチで、**`error` 以外は何を書いても `warning`** になる
（`severity()` は rule config の severity が `error` のときだけ `error` を返す）。
`revive-severity-info` も `revive-settings` とバイト一致する。
設定文字列をそのまま流すのが素直な実装で、それは間違い ——
**severity を比較するゲートでしか見えない**。

gosec は `filterIssues` の `i.Severity >= severity && i.Confidence >= confidence` で、
fixture は **2 軸で順序が食い違う 4 ルール**にした（G104 low/high、G401 medium/high、
G404 high/medium、G101 high/low）。`severity` を上げると G104 から消え、
`confidence` を上げると G101 から消える。**同じ 4 件で 2 つのキーが別々に読める。**

どちらも 17 ケース 50 findings が一発一致で、**バグは 0**。
それでも既定値は 2 つとも新しく測れるようになった:
`gosec-default-rules` は `includes` を**書かない**唯一のケース（既存の `cases/gosec` は
35 ルールを名指しする）で、`revive-settings` は `severity` を書かない唯一のケース。

#### 5. ついでに落ちた 1 件と、k9s に残っている 3 件

gocritic の `enabled-tags` を直したので、**k9s の設定で初めて両ツールを突き合わせられる**
ようになった（k9s は `corpus/repos.json` の tier に入っていない —— それが、この規模の
recall バグが OSS ゲートを緑のまま通り抜けた理由でもある）。
`./internal/...` を k9s 自身の gocritic 設定で走らせると、**guff だけが 5 件**出した。
うち 2 件は 1 行で直った:

**`boolExprSimplify` は `if` / `for` の条件そのものには当たらない。**
`VisitExpr` の門は `typep.HasBoolKind`、つまり **`types.Basic` の kind が `Bool`
ちょうど**であることで、**`UntypedBool` は別の kind なので落ちる**。比較式が typed に
なるのは何かが型を与えたときだけなので:

| 形 | 条件の型 | 上流 |
|---|---|---|
| `_ = x+1 > y` | `bool`（代入が既定型を与える） | 報告する |
| `if x+1 > y && ok` | `bool`（`ok` が typed） | 報告する |
| `switch { case x+1 > y: }` / `f(x+1 > y)` | `bool` | 報告する |
| **`if x+1 > y`** | **`untyped bool`** | **報告しない** |
| **`for x > y-1`** | **`untyped bool`** | **報告しない** |

guff の `type_is_boolean` は `info().contains(IS_BOOLEAN)` で、これは
`UntypedBool` にも真になる。`kind() == BasicKind::Bool` に締めたら k9s の 2 件が消え、
`cases/gocritic` のゴールデンは**逆に 1 行増えた** —— `extras.go` に足した
`if a+1 > b && x` の側は上流も報告するからで、**同じ 1 行の修正の両側**が
1 つのゴールデンに載っている。

残る 3 件は**このセッションでは直していない**。次に開ける人のために再現手順ごと置いておく:

```
cd corpus/cache/k9s
golangci-lint run -c <gocritic だけを有効にした k9s の設定> --path-mode abs ./internal/...
```

- `rangeValCopy` が `_test.go` の 3 か所で guff だけ出る
  （`internal/render/{container_int_test,node_int_test,table_test}.go`）。
  golangci-lint は同じファイルの `appendAssign` は出しているので、
  **テストファイルを見ていないのではない**
- `importShadow` が `internal/config/alias_test.go:102` で guff だけ出る
  （`shadow of imported from '…/internal/view/cmd' package 'cmd'`）

**k9s を tier に入れる**のが本当の直し方で、それは Phase 5 の仕事そのもの。

#### 6. 直していないもの: gosec の `G407`

golangci-lint は gosec の settings ブロックがあると **`excludes` に `G407` を
無条件で足す**（securego/gosec#1211 の回避）。guff は G407 を実装していないので
**偶然一致している**だけで、規則として一致しているわけではない。
G407 を移植する日には、この append も一緒に移植すること。

**ゲート**

- `./compat/golden/run.sh` — **77 ケース**（新規 34）。ratchet 4 本は baseline のまま。
  `cases/gocritic` は 164 → 165 findings（`boolExprSimplify` の修正で足した
  `if a+1 > b && x`）
- `cargo test --workspace` — 緑。新規単体テスト 6 本
  （`errcheck_assert_positions_and_pruning_match_upstream`、
  `govet_enable_is_checked_before_disable`、
  `govet_default_set_is_not_every_analyzer`、
  `gocritic_selector_keys_follow_infer_enabled_checks`、
  `gocritic_default_checks_are_exactly_the_untagged_ones`、
  `gocritic_every_implemented_check_has_tags`）
- `./compat/run.sh --oss --tier pr,nightly` — 8 ターゲット緑。
  **consul は guff=258 / golangci=255 / P=98.8% / R=100%** で 8 本目から 1 件も動かず、
  allowlist も consul の 3 件のまま。errcheck の枝刈りと括弧、gocritic の `enabled-tags`、
  govet の優先順はどれも**この 8 リポの config では発火しない**。実測: `corpus/cache/*/`
  のリポジトリ直下 config で gocritic の `enabled-tags` を書いているのは **k9s だけ**で、
  tier の 6 リポ（gin / caddy / helm / consul / grafana / containerd）はどれも書いていない
  —— つまりこのキーの意味論は**どのゲートでも一度も比較されていなかった**（上の 5 節）
- `./compat/run.sh --isolate` — **116 ターゲット**全部緑
- `compat/coverage.py observe && report` — 台帳の数字は**動かない**（543 / 1 / 3）。
  8 本目と同じ理由で、これは想定どおり

**次にやること**

1. **Phase 4 は閉じた**（ランナー側 8 本目 / linter settings 9 本目）。残っている settings キーは
   どれも finding-set を変えないか、既に別のゲートで見えているもの。次に開けるのは
   **Phase 5（コーパスの多様化）**で、現行 8 リポが踏めていない形は §2 に列挙してある ——
   generics、cgo、build tags、`go.work`、`vendor/`、`embed`、テストのみパッケージ、
   アセンブリ、非 ASCII 識別子、巨大生成ファイル。
   **9 本目の 10 バグのうち 7 つは「既定値」か「セレクタ文法」**で、8 本目の 3 バグと同じ
   2 種類だった。Phase 5 が探すのは 3 種類目（**入力の形**）になる。
   **最初に入れる候補は k9s** —— 上の 5 節の 3 件がそこにあり、`corpus/repos.json` の
   tier に入っていないこと自体が今回いちばん大きな recall バグを隠していた。
2. **config の validate**（7・8 本目からの持ち越し）。今回さらに 2 件増えた:
   gocritic の `enable-all` + `enabled-tags` と `disable-all` + `disabled-checks` は
   上流が `validateOptionsCombinations` で拒む組み合わせで、`disable-all` だけを書いて
   何も enable しない config も拒まれる。guff はどれも受理して黙って走る。
3. **gosec G304 の実装**（7 本目からの持ち越し）。実装したら
   `exclusionsem/presets` に fixture を戻し、EXC0010 が preset ケースで比較されるようにする。
   G407 も同じ箱（上の 5 節）。
4. 6 本目からの持ち越し: §5 の台帳の残り 6 件、インターフェースメソッドのレシーバ配線、
   gosec の他の DEFERRED、SA9008 / SA5011 の σ、govet の未実装 16 pass。
### 2026-08-12（10 本目）— Phase 5 に着手: コーパスに 2 リポ足して 6 バグ、そして「形」の台帳

**やったこと**

9 本目の「次にやること 1」に従い **Phase 5（コーパスの多様化）**を開けた。
k9s と cobra を tier に入れ、grafana を 2 モジュールに広げ、
**「どの形の入力がどのゲートにも当たっていないか」を測る台帳**（`corpus/shapes.py`）を作った。

#### 0. まず測った —— tier が実際に解析している「形」

Phase 5 の §2 は踏めていない形を列挙していたが、**その列挙自体が推測**だった。
`go list -e -json` を各ターゲットの**実 package パターン**で回した結果は §2 の表のとおりで、
cgo / vendor 配下 / 非 ASCII 識別子が 0、**マルチモジュールを踏んでいるターゲットは 1 つも無い**。
チェックアウトを数えると grafana は 47 モジュール・kubernetes は 36 モジュールに見えるが、
**ゲートが見るのはパターンが選ぶ集合だけ**で、どちらも 1 モジュールしか解析していなかった。

#### 1. k9s —— 14 件の差分は 4 個のバグだった

`P=97.8%`（guff 650 / golangci 636 / guff-only 14）から始まり、**allowlist 0 件で 636/636** に到達。

**(1) 同じ linter が `enable` と `disable` の両方に書いてある。**
k9s の config は `disable: [staticcheck]` と `enable: [..., staticcheck, ...]` を**両方**持つ。
上流はこれを **disable 勝ち**として扱い staticcheck を 1 件も報告しない。guff は enable 勝ちで、
k9s に実在する 9 件を撃っていた。最小再現で `default` を standard / none / all の 3 通り、
2 つのリストの**記述順も入れ替えて**測ったが、答えは常に disable 勝ちだった。

面白いのは **9 本目が govet の同名キーで逆の結論を出している**こと
（`enable` を先に見るので両方に書かれたアナライザは有効）。
**綴りが同じキーが、階層違いで逆に解決する。** `config.rs::resolve_names` は
「base から disable を引き、その後 enable を**無条件で** push し直す」形だったので、
両方に載っている名前が最後に必ず復活していた。

**(2) nolintlint の findings だけが除外フィルタの外にいた。**
k9s の `linters.exclusions.paths` は `internal/x` を含む。これは**アンカーされていない正規表現**なので
`internal/xray/` 以下に丸ごと当たる。上流はそのツリーの findings を 50 件ほど全部落とし、
guff も落とす —— **nolintlint の 1 件を除いて**。`exclude.rs::apply` は path / text / rules の
3 フィルタを掛けた**後**に `filter_issues` を呼び、そこで初めて nolintlint の findings が
**生まれる**ためだった。上流では nolintlint は普通の linter で、findings はプロセッサが走る前から
存在する。3 フィルタを生成後にもう一度掛ける形にした（既に通ったものには冪等）。

**(3) `skipTestFuncs` は未実装の設定ではなく、既定値の取りこぼしだった。**
`rangeValCopy` と `rangeExprCopy` は go-critic の 107 チェッカのうち**この 2 つだけ**が
`skipTestFuncs` を持ち、その**既定は true**。`EnterFunc` が unit test 関数で false を返して
サブツリーごと刈る。`isUnitTestFunc` はファイル名を一切見ず、**名前と署名**だけを見る
（`Test` 接頭辞 / `*testing.T` 1 個 / 戻り値なし）。guff の DEFERRED 一覧には
`sizeThreshold` は載っていたが `skipTestFuncs` は無く、
**「設定を配線していない」ではなく「既定の挙動が違う」**側だった。

**(4) `importShadow` は上流より広く走査していた。**
`astwalk.localDefWalker` が「定義」と見なすのは `AssignStmt(DEFINE)` と `GenDecl` の
ValueSpec だけで、**どちらの case も `return false` で終わる**。上流に食わせた実測:

| 形 | 上流 | guff（修正前） |
|---|---|---|
| `for os, strings := range m` | 報告しない（RangeStmt は AssignStmt ではない） | **報告する** |
| `f = func() { os := 1 }`（非 define 代入） | 報告しない（降りない） | **報告する** |
| `var g = func() { os := 1 }`（GenDecl） | 報告しない（降りない） | **報告する** |
| `func() { os := 1 }()`（それ以外の経路） | 報告する | 報告する |
| `func C() (os int)`（名前付き戻り値） | **報告する** | **報告しない** |

最後の 1 行は recall 側で、k9s には出ていない。**上流を読んだから見つかった**もので、
`walkSignature` が params → results → recv を回すのに guff は recv → params しか回していなかった。

#### 2. cobra —— `go 1.15` が引き当てた printf のバグ

コーパスで最も古い `go` ディレクティブ（他は全部 1.24 以上）が目的だったが、
出たバグは go バージョンとは無関係だった。`P=98.1%`（160 / 157）の 3 件はすべて
`fmt.Sprintf("… %-36[1]s …")` で、guff が `%-36[` を「未知の verb `[`」と読んでいた。
`fmtstr.ParsePrintf` は `parseIndex` を **3 か所**で呼ぶ —— フラグの直後、`.` の直後
（`parsePrecision` の中）、そして **`indexPending` でなければ verb の直前でもう一度**。
guff は最初の 1 か所しか見ていなかったので、`%[1]s` は通るのに `%-36[1]s` が通らなかった。
`*` が保留中の index を吸収する（`%[1]*d`）ところまで含めて移植した。

#### 3. grafana を 2 モジュールに

`./pkg/...` に `./apps/advisor/...` を足し、**1 回の実行が go.work の 2 モジュールを跨ぐ**形にした
（837 パッケージ / 2 モジュール）。差分は出ず、ill-typed はむしろ 30 → 27 に減った。
「差分が出ないこと」自体が測定結果で、それまでこの形は**一度も測られていなかった**。

#### 4. 非 ASCII は fixture で埋め、godox の位置バグが出た（`cases/nonascii`）

コーパスに無い形なので golden 側に置いた。狙いは**単位系が 2 つ混在している**こと:
finding の **column はバイト**（go/token）、lll の行長は**ルーン**
（`utf8.RuneCountInString`）、godox のメッセージ切り詰めは**ルーン 40**。
fixture の finding は全部多バイト文字の**後ろ**に置き、34 ルーン / 94 バイトのかな行を 1 本入れて
lll がどちらで数えるかを固定した。

ここで **godox の位置が 2 つとも違っていた**。しかも**非 ASCII とは無関係**で、
ASCII だけの最小例でも全件ずれる:

| | 上流 | guff（修正前） |
|---|---|---|
| column | コメント開始桁 **+1**（1 桁目のコメントは 2） | 常に 1 |
| ブロックコメント途中行の line | **コメント開始行**（`/*` の行） | キーワードのある行 |

godox 自身の `Message.Pos` は `fset.Position(comment.Pos())` で**常にコメント先頭**であり、
行オフセットは godox が自前で組み立てるメッセージ文字列の中にしか入らない。
golangci-lint はその文字列を捨てて `i.Pos` から作り直すので、**行オフセットは消える**。
桁の `+1` は golangci 側のラッパが足している。
**godox は 2026-08 に caddy で panic を直したチェッカ**（§2 Phase 2）だが、
あのとき合わせたのは finding の集合だけで、`compat/normalize.py` の比較キーは column を見ない。
**golden に載せて初めて位置が比較された。** §5 が「残りの linter にも同種の column バグが
あると考えるのが妥当」と書いていたとおりだった。

#### 5. ハーネスのバグ: `normalize_path` が実在するディレクトリを剥がしていた

`cases/nonascii` を足したら guff だけ `nonascii/nonascii.go` を `nonascii.go` と報告した。
guff を直接動かすと正しいので、犯人は `compat/normalize.py` のほうだった:
「golangci はモジュールのディレクトリ名を前置することがある」ための剥がし処理が、
**root の basename と同じ名前のパッケージディレクトリ**を無条件に食っていた。
golangci 側は絶対パスで報告するので早い分岐に入り、guff 側だけが剥がされる。
「root 直下に実在するならそのまま返す」判定を**先**に持ってきた
（剥がすのは実在しないときだけ ＝ 前置が偽物のときだけ）。
**ケースの名前とパッケージの名前が衝突するまで誰も踏まなかった**種類のバグで、
Phase 5 が新しい形を入れると出てくるのはこういうものでもある。

#### 6. 直していないもの: k9s の ill-typed 1 件

`internal/dao` が guff の型検査に落ちる（`go build` は通る）。`accessors` の map リテラル
24 エントリが全部 `cannot use *Workload value as Accessor value` になる。
**findings は 636/636 で一致している**ので今は見えないが、型依存のアナライザがこのパッケージで
丸ごと落ちている＝ Phase 1 が言う「差分に出ない失敗」の予備軍である。
他の 6 ターゲットと同じく baseline に記録した（k9s 1 / grafana 27 / consul 7 / kubernetes 10）。

**次に開ける人のために、再現しなかった縮小を全部置いておく**（どれも `go build` が通り、guff も黙る）:

1. 埋め込みインターフェースからのメソッド昇格（3 段の struct 埋め込み経由）
2. `Workload → Table → Generic → NonResource` の埋め込み鎖をそのまま写したもの
3. 前方参照（`Accessors` 型が使用行より後ろ、`Accessor` が別ファイル）
4. `Factory` が `Get` / `List` を**別シグネチャで**持ち、`NonResource` がそれを埋め込む形

パッケージ内に probe ファイルを置くと `_ Accessor = (*Workload)(nil)` も
`Accessors{client.WkGVR: new(Workload)}` も**通る**。同じ型の対が `accessor.go` では落ちて
後ろのファイルでは通るので、残る差はチェック順序だと思われる。
**手で縮小する限界に当たっている** —— これは Phase 6 の `compat/reduce.py`
（delta-debugging）が存在する理由そのもので、次にこれを開けるなら**先に縮小器を作るほうが速い**。

**ゲート**

- `./compat/golden/run.sh` — **78 ケース**（新規 `nonascii`）。`cases/gocritic` は
  165 → 172 findings（`testfuncs.go` を追加、`extras.go` に importShadow の 3 形）。ratchet 4 本は baseline のまま
- `./compat/run.sh --oss --tier pr,nightly` — **10 ターゲット**緑（新規 k9s / cobra）。
  k9s 636/636、cobra 157/157、grafana は 2 モジュールで 0/0。consul の allowlist 3 件は据え置き
- `./compat/run.sh --isolate` / `./compat/filesets.sh --tier pr,nightly` — 緑
- `./corpus/shapes.py check` — 必須 9 形すべて gated ターゲットが踏んでいる
  （`cgo` と `nonascii` は `EXCLUDED`）
- `compat/coverage.py observe && report` — **543 / 1 / 3 のまま動かない**。
  8・9 本目と同じ理由で想定どおり: Phase 5 が増やすのは check ではなく
  **同じ check に通る入力の形**なので、この台帳の数字は原理的に動かない。
  **動いた指標は golden の 165 → 172 と OSS の 8 → 10 ターゲットのほう**
- `cargo test --workspace` — **3,044 passed / 0 failed**（ignored 11）。新規単体テスト 2 本
  （`linter_in_both_enable_and_disable_is_disabled`、`gocritic_range_copy_skips_unit_test_funcs`）。
  printf の 5 形は既存の `printf_allows_stringer_and_composites`（ok2.go は 0 件であること）が拾う。
  なお `gocritic_range_copy_skips_unit_test_funcs` は **`testdata/gocritic/stub/testing/` を足さないと通らない** ——
  単体テストのハーネスは stdlib を stub でしか解決せず、`testing` が無いと
  `*testing.T` が undefined になって `isUnitTestFunc` が常に false になる。
  **golden 側（本物のツールチェーン）は 172/172 で一致していたので、これは guff ではなくハーネスの穴**

**次にやること**

1. **Phase 5 の残り**: 台帳が `EXCLUDED` 扱いにしていない形はもう無いが、
   **踏んでいる形の「濃さ」は測っていない**。generics は grafana の 66 ファイルで踏んでいる
   ことになっているが、それが型パラメータ制約の何を通しているかは別問題。
   次の 1 リポを選ぶなら **ent（generics + codegen）か tailscale（cgo + build tags）**で、
   cgo を入れるなら §2 の `EXCLUDED` の判断（C ツールチェーンを CI の前提にしない）を
   先に覆すこと。
2. **k9s の ill-typed**（上の 6）。`compat/reduce.py` を先に作るほうが速い ＝ 実質 Phase 6 の着手。
3. 9 本目からの持ち越し: config の validate、gosec G304 / G407 の実装。
4. 6 本目からの持ち越し: §5 の台帳の残り 6 件、インターフェースメソッドのレシーバ配線、
   SA9008 / SA5011 の σ、govet の未実装 16 pass。

### 2026-08-12（11 本目）— Phase 5 の二度目の測定: 「generics は covered」の中身を測ったら 8 バグ

**やったこと**

10 本目の「次にやること 1」——**踏んでいる形の「濃さ」は測っていない**——をそのまま開けた。
台帳は `generics` を 7 ターゲットが covered と言っていたが、
**その covered が何でできているか**は誰も見ていなかった。

#### 0. サブ形を数えた —— `genericrecv` は gated ターゲット 8 個中 0 個だった

`corpus/shapes.py` に 3 列足した（どれも構文だけを見る下限値で、census ではない）:

| 形 | 意味 | 実測（gated 8 ターゲット） |
|---|---|---|
| `genericrecv` | ジェネリック型のメソッド `func (x T[P]) M()` | **consul 21 / grafana 18、pr tier は 0** |
| `genericunion` | `~T` / `A \| B` の型集合 | caddy 4 / consul 8 / grafana 36 |
| `genericalias` | ジェネリック型エイリアス（go1.24） | **全ターゲット 0** |

`genericrecv` を `REQUIRED` に入れた（今回のバグの 3 つがこの形にしか出ない）。
**ただし「covered」は依然として弱い保証**である: consul と grafana は 21 + 18 ファイル
持っているのに今回のバグを 1 件も撃たなかった —— **それらの実 config が
revive の `exported` を有効にしていない**からで、形とチェックの積を測っているのは
golden tier のほうだけである。だから新ケース `cases/generics` を置いた。

#### 1. controller-runtime を足した —— 39 差分 = 8 バグ

v2 config を持つ generics + codegen リポとして `kubernetes-sigs/controller-runtime@v0.24.1`
を入れた（ent は v1 config でハーネスの前提を満たさない）。初回 **P=88.5%**（guff 339 / golangci 300）。

**(1) revive の受け手の綴りが 3 通り全部違っていた（24 件）。**
`receiver_type_key` は上流 `internal/typeparams.ReceiverType` の移植のつもりで、
実際には (a) `*` を**付けたまま**返し、(b) 型引数を剥がさず、(c) 想定外の形を
**Rust の `{:?}` で Debug 出力**していた。結果:

| 受け手 | 上流 | guff（修正前） |
|---|---|---|
| `func (p *Plain) Method()` | `exported method Plain.Method …` | `*Plain.Method` |
| `func (b *Box[T]) Get()` | `Box.Get` | `*IndexExpr(IndexExpr { x: Ident(Ident { name_pos: Pos(91), …` |
| `func (h *hidden[T]) Exported()` | **報告しない**（private receiver） | 報告する |

3 番目が効きの大きいところで、Debug 文字列は `I` で始まるので
`ast.IsExported` が真になり、**非公開のジェネリック型のメソッドが全部誤検出**になっていた。
上流は `*` を剥がしてから `IndexExpr` / `IndexListExpr` を開き、それ以外は
`"invalid-type"` を返す（`func (t (*T)) M()` —— go vet が通す形 —— はこれに落ちる。実測して確認した）。
`confusing-naming` だけは**別の関数**（`getStructName`）で、フォールバックが `_`、
`IndexListExpr` を開かない（＝ 2 パラメータのジェネリック型のメソッドは
パッケージ関数と同じ箱に入る）ので、そちらは専用の移植にした。

**(2) revive `var-declaration` は ValueSpec の下に降りてはいけない（5 件）。**
上流の visitor は `*ast.ValueSpec` の case を**どの経路でも `nil` で返す**ので、
**var 初期化子の中は一切見ない**。ginkgo のスイートは丸ごと
`var _ = Describe("…", func() { … })` の中にあるため、上流はそこの `var count uint64 = 0` を
1 件も報告しない。guff は `shared_walk`（「刈らないルール専用」と自分で書いてある）に
相乗りしていたので全部報告していた。刈る以上そこには乗れないので、
自前の pruning walk に戻した。最小再現は 15 行（`var fn = func() { var x uint64 = 0 }`）。

**(3) gocritic `newDeref` は型を見ていなかった（2 件 + 4 件の recall）。**
`*new(T)` の示唆を**引数の綴りだけ**から作っていた（`other => format!("{other}{{}}")`）。
上流は `lintutil.ZeroValueOf` に**型**を渡す。24 形を上流に食わせて測った結果:

| 形 | 上流 | guff（修正前） |
|---|---|---|
| `*new(T)`（型パラメータ） | **報告しない**（go-critic #1272） | `T{}` |
| `*new(int32)` | `int32(0)` | `0` |
| `*new(MyInt)` | `MyInt(0)` | `MyInt{}` |
| `*new(*MyStruct)` | `(*MyStruct)(nil)` | `*MyStruct(nil)` |
| `*new([]int)` / `map` / `struct{…}` | `[]int(nil)` など | **何も報告しない**（`expr_text` が型構文を描画できず None） |
| `*new(chan int)` / `func()` | 報告しない | `chan int{}` |
| `*new(complex128)` | `%!s(PANIC=String method: runtime error: invalid memory address or nil pointer dereference)` | `0` |

最後の行は上流のバグで、`zv` が nil のまま `*ast.CallExpr` に包まれ、`fmt` が
String メソッドの panic を捕まえてこの文字列を書く。**finding は出る**ので、
落とすと recall 損失になる。文字列ごと再現した。描画は `expr_text` ではなく
`node_text`（go/printer）に替えた —— そこの doc コメントが最初から
「ノードを埋め込むメッセージはこちらを通せ」と書いてあった。

**(4) errorlint の allowed-errors は「センチネル」ではなく「対」だった（2 件 + recall）。**
guff はセンチネル 4 つを**どこから来た error でも**免除していた。上流は
`(センチネル, それを返した関数)` の**対**で 64 行の表を引き、`err` に代入した
呼び出しを識別子をまたいで遡る（`assigningCallExprs`）。表と機構を丸ごと移植した。
効果は両側にある: `net/http.ErrServerClosed`（表にあるが guff の 4 つには無い）が
差分として消え、同時に**`err == io.EOF` が許可外の関数の戻り値でも黙っていた**recall 損失も消えた。
ついでに value switch の報告位置が違っていた（上流は
`problematicCaseClause.Pos()`＝ `case` の位置、guff は `switch`）。

**(5) SSA を読む 16 個の analyzer がメソッドを 1 つも見ていなかった。**
`nolintlint` が `//nolint:nilerr` を「未使用」と言ったのが糸口。
`BuildIrResult::src_funcs` は `Package.members`（＝パッケージレベル関数）だけで、
**メソッドは入らない**。上流の `buildssa` / `buildir` の `SrcFuncs` は
AST の FuncDecl 全部（メソッド込み）である。しかも guff 側は
`buildir_src_methods` 設定に依存していて、この設定は
**contextcheck が有効なときだけ真**になる ——
つまり**有効な linter の集合によって finding が変わる**状態だった。
`src_funcs_with_methods()`（§7 のとおり SA5011 だけは σ が無いので据え置き）に
16 か所を切り替えた: nilerr / nilnesserr / zerologlint / callcheck / purity /
SA1015 / SA1025 / SA2002 / SA2003 / SA4009 / SA4010 / SA4012 / SA4023 /
SA5000 / SA5007 / SA5010。**pr tier の 5 ターゲットは全部 100% のまま**（誤検出は増えなかった）。

**(6) ドット import の使用記録がパッケージ単位だった（ill-typed 44 → 16）。**
`dot_imported: HashMap<PackageId, ObjectId>` は「Go の `dotImportMap` の簡略化」と
自分でコメントしていたが、**同じパッケージを 2 つのファイルがドット import すると
片方の `PkgName` が上書きされ**、負けたファイルが
`"github.com/onsi/ginkgo/v2" imported and not used` になる。
ginkgo/gomega を使うリポでは**テストパッケージが軒並み ill-typed**になり、
型に依存する analyzer が丸ごと落ちる（Phase 1 が言う「差分に出ない失敗」）。
Go は `(fileScope, name)` で持つので、そのとおりに直した（1 名前 1 エントリ）。
最小再現は 2 ファイル 8 行。

**(7) SA1019 の位置とメッセージ（golden の ratchet が missing 7/extra 9 → 5/7）。**
上流は `report.Report(pass, sel, …)` に**セレクタ式全体**を渡すので位置は `x` の先頭
（`lib.OldFunc` なら `lib`）。guff は選択された名前のほうを指していた。
さらにメッセージ本文: 上流は `strings.Replace(alt, "\n", " ", -1)` だけで **trim しない**ので、
`doc.Text()` 由来の**末尾スペースがそのまま出る**。guff は trim していた。
golden の比較は改行しか落とさないので、この 1 文字が 2 行分の差分だった。

**(8) 非推奨の**インターフェースメソッド**が別パッケージから見えなかった。**
依存ソースの遅延スキャンが `Decl::FuncDecl`（＝レシーバ付きの具象メソッド）しか
methods 表に入れていなかった。`type P interface { // Deprecated: … \n M() }` を
インポート側から呼んでも黙る。TypeSpec が InterfaceType のとき
メソッドの doc も拾うようにした。

#### 2. `cases/generics` —— 形とチェックの積を golden で固定した

コーパスに無い形は fixture で埋める（`nonascii` の前例）。
`crates/guff-lint/tests/testdata/generics/generics.go` + `compat/golden/cases/generics`（8 件）。
revive（exported / receiver-naming / unexported-return / var-declaration）、
gocritic の `newDeref`、nilerr を**位置とメッセージまで**比較する。
ジェネリック型のメソッド、2 パラメータの受け手、非公開ジェネリック型の公開メソッド、
型パラメータの `*new(T)`、メソッド本体の nilerr が入っている。

#### 3. 直していない: guff の型検査が落ちるジェネリックの 2 形

fixture を書いていて出た。**どちらもパッケージ全体が ill-typed になる**
＝ 型依存の analyzer が丸ごと黙る側の欠陥である。

```go
type Box[T any] struct{ v T }
type Alias[T any] = Box[T]        // guff: undefined: T   (go1.24 のジェネリック型エイリアス)

type Number interface{ ~int | ~float64 }
func Sum[T Number](xs []T) T {
    var total T
    for _, x := range xs { total += x }   // guff: operator ADD not defined on operand
    return total
}
func Less[T Number](a, b T) bool { return a < b }  // guff: operator LSS not defined on operands
```

2 つ目は `crates/guff-types/src/predicates.rs` が自分で書いている
「Type-set-aware variants (`allX`) are deferred」そのもの。上流の `Checker.binary` は
`allNumeric` / `allOrdered`（型集合の全項が満たすか）を引く。
**ジェネリックな算術を書くコードは、今の guff では丸ごと解析されない。**
fixture はこの 2 形を避けて書いてあり、避けた理由はファイル内のコメントにも残してある。

#### 4. controller-runtime は `weekly` に置いた（gated tier ではない）

残差分 8 件（うち 5 件は上の (8) と同族で、`cluster.Cluster` が**別パッケージの
インターフェースを埋め込んでいる**ため、選択の受け手（`Cluster`）と
メソッドの宣言元（`recorder.Provider`）が食い違うケース。
`guff-types` がインターフェースメソッドにレシーバを繋いでいない §7 の欠落と同じ根）と、
bodyclose 1 件・unparam 1 件が残っている。
**allowlist を 8 件足して pr tier に入れるより、緑の gated tier を保ったまま
weekly に置くほうが正直**と判断した（k9s / cobra は allowlist 0 件で入っている）。
ill-typed は baseline に 16 で記録した。移すのは残り 8 件が消えたとき。

**ゲート**

- `./compat/golden/run.sh` — **79 ケース**（新規 `generics`）。
  `staticcheck-sa` の ratchet を **missing 7 → 5 / extra 9 → 7** に下げた（SA1019 の 2 行）
- `./compat/run.sh --oss --tier pr,nightly` — 10 ターゲット緑
  （k9s は 5 回中 1 回だけ goconst で 7 件ずれた。上記 2c）（consul の allowlist 3 件は据え置き）
- `./compat/run.sh --isolate` — 116 ターゲット緑
- `./compat/filesets.sh --tier pr,nightly` — 8 ターゲット一致
- `./corpus/shapes.py check --offline` — **必須 10 形**（`genericrecv` を追加）
- `cargo test --workspace` — 緑

**次にやること**

1. **型検査のジェネリック 2 形**（上の 3）。`allX` の型集合対応が本体で、
   これが入るまで「generics covered」は名ばかりである。ジェネリック型エイリアスも同じ枠。
2. **controller-runtime を pr tier に上げる**。残り 8 件のうち 5 件は
   §7 の「インターフェースメソッドにレシーバが繋がっていない」を直せば同時に消える見込み。
3. `manager.Manager has no field or method GetCache` 系の ill-typed 16 件。
   **最小再現は取れていない**（別パッケージのインターフェース埋め込み・同名メソッドの
   重複宣言・in-package テストでの拡張、いずれも単体では再現しない）。
   10 本目の k9s と同じ壁で、**`compat/reduce.py`（Phase 6）を先に作るほうが速い**。
4. 10 本目からの持ち越し: config の validate、gosec G304 / G407。
   6 本目からの持ち越し: §5 の台帳の残り 6 件、SA9008 / SA5011 の σ、govet の未実装 16 pass。

### 2026-08-12（12 本目）— 型集合を見る型検査: `allX` / untyped 定数 / ジェネリック型エイリアス

**やったこと**

11 本目の「次にやること 1」をそのまま開けた。11 本目は「`generics` covered の中身」を測った。
12 本目が測ったのは**その中身が guff の型検査を通っているか**で、通っていなかった。
`go build` の通るジェネリック算術は 1 行残らず落ち、**落ちるとパッケージ全体が
ill-typed** になる ＝ 型に依存する analyzer が丸ごと黙る。
Phase 1 が「差分に出ない失敗」と呼んでいるものそのもので、
**finding が 0 件になる欠陥は finding の差分では見つからない**。

#### 1. `allX` —— `isX` は型パラメータの中を見ない（11 箇所）

上流の `predicates.go` は 2 つの族を持つ。`isX` は `t.Underlying()` で止まり、
`allX` は**型パラメータの型集合の全項**に対して `isX` を確かめる
（項が 1 つも無い ＝ `any` は `f(nil)` を呼ぶので**常に偽**）。
guff は前者しか持たず、後者を「呼ばれたときに足すのは簡単」と
`predicates.rs` のモジュールコメントで先送りしていた。実際には
**演算子のほぼ全部**が上流では `allX` を引いている:

| 呼び出し元 | 上流の述語 | guff（修正前） |
|---|---|---|
| `unaryOpPredicates`（`+` `-` `^` `!`） | `allNumeric` / `allInteger` / `allBoolean` | `isNumeric` … |
| `binaryOpPredicates`（`+` `-` `*` `/` `%` `&` `\|` `^` `&^` `&&` `\|\|`） | `allNumericOrString` / `allNumeric` / `allInteger` / `allBoolean` | 同上 |
| `comparison` の `< <= > >=` | `allOrdered` | `isIntegerOrFloat \|\| isString` |
| `shift` の左辺・右辺、`updateExprType` の shift-lhs | `allInteger` | `isInteger` |
| ゼロ除算ガード | `allInteger` | `isInteger` |
| `if` / `for` の条件、`&&` の両辺 | `allBoolean` | guff の `Checker::all_boolean` は名前だけで、中身は `Underlying` 1 段（自分でそう書いてある） |
| `x++` / `x--` | `allNumeric` | `isNumeric` |
| 添字 `s[i]` | `allInteger` | `isInteger` |
| `min` / `max` | `allOrdered` | `isOrdered` |
| `append([]byte, string...)` | `allString` | `isString` |

`all_basic` は `TypeSet::is`（4 本目から在る）に乗るだけで書けた。
`all_unsigned` だけは呼び出し元が無い —— 上流でそれを引くのは
「符号付きシフト量は go1.13 以降」の版判定で、guff にその判定が無いからである。
族として揃えて置いた（移植であって最小実装ではない）。
先送りのコメントが「trivial to add when first called」と言っていたのは正しく、
**間違っていたのは「まだ呼ばれていない」のほう**である。

ground truth は `go build`（go1.26）。肯定 19 形・否定 14 形を実際に食わせて
`crates/guff-types/tests/generic_ops.rs` に落とした。否定側が要るのは
`allX` が**全項**を要求する述語だからで、`~int | ~string` は `+` を許して `-` を許さない、
`~int | ~bool` は `<` も `min` も許さない、`any` は何も許さない。

#### 2. untyped 定数 → 型パラメータ（`underIs`）

`allX` を入れても `a++` が通らなかった。合成される `a + 1` の `1` を
型パラメータに変換できないためで、上流 `implicitTypeAndValue` の
`*TypeParam` の枝（`underIs` で**型集合の全項が受け入れるか**を見て、
受け入れるなら**基底型ではなく型パラメータ自身**を、**丸めた値なしで**返す）が
無かった。guff は `InvalidUntypedConversion` を返す 1 行で塞いでいた。
`var total T = 0` / `total += 1` / `a++` が全部これに当たる。
`under_is` は `under.rs` に既にあった。

#### 3. ジェネリック型エイリアス（go1.24）

`type Alias[T any] = Box[T]` が `undefined: T`。`type_decl` のエイリアス枝が
**RHS を解決してから** `new_alias` していたので、型パラメータを吊るす先が
その時点で存在せず、スコープも開いていなかった。上流は
`newAlias(obj, nil)` → `openScope` → `collectTypeParams(&alias.tparams, …)` →
RHS → `alias.fromRHS = rhs` の順である。同じ順に組み替え、
`alias_set_rhs`（`from_rhs` を埋めて `actual` を貼り直す）を足した。
`collect_type_params` は Named 決め打ちだったので Alias にも振るようにした。
spec の「エイリアス宣言の RHS に自分が宣言した型パラメータは書けない」
（`type Bad[P any] = P`）も同時に入れた。

#### 4. エイリアス実体の TypeName に package が無かった（revive `unexported-return`）

fixture に `type hiddenAlias[T any] = hidden[T]` と、それを返す公開関数を足したら
golden が 1 件 missing になった。`new_alias_instance` が実体用の TypeName を
`new_type_name(oarena, name, None)` で作っており、**package を持ち越していなかった**
（上流は `NewTypeName(pos, orig.obj.pkg, orig.obj.name, nil)`）。
`Object.Pkg() == nil` は呼び出し側が「組み込み型」を綴る言い方そのものなので、
revive の `exportedType` は最初の `case obj.Pkg() == nil:` で素通りし、
**非公開ジェネリックエイリアスの実体が全部 exported 扱い**になっていた。
Named の実体は origin の obj を使い回していたので、エイリアスだけの穴だった。

#### 5. fixture に 2 形を戻した

`crates/guff-lint/tests/testdata/generics/generics.go` は 11 本目に
「型検査が通るようになったら戻すこと」と書いて 2 形を避けてあった。戻した:
`Sum`（`var total T = 0` + `+=`）、`Max`（`<`）、`Alias` / `hiddenAlias` /
`NewHidden`。golden は 8 → **10 件**に増え、増えた 2 件が
revive の `var-declaration`（型パラメータの変数に対する報告）と
`unexported-return`（エイリアス越し）である。
`corpus/shapes.py` の `genericalias` は `EXCLUDED` に移した ——
どのターゲットでも 0 と測れており、`nonascii` と同じく fixture で埋める形だから。

**ゲート**

- `./compat/golden/run.sh` — **80 ケース**緑（ratchet は据え置き: `staticcheck-sa` missing 5 / extra 7、`staticcheck-s` missing 2、`staticcheck-st` missing 10、`revive` missing 1 / extra 3）
- `./compat/run.sh --isolate` — 116 ターゲット緑
- `./compat/run.sh --oss --tier pr,nightly` — 10 ターゲット緑
  （k9s は 5 回中 1 回だけ goconst で 7 件ずれた。上記 2c）
- `./compat/filesets.sh --tier pr,nightly` — 8 ターゲット一致
- `./corpus/shapes.py check --offline` — 必須 10 形
- `cargo test --workspace` — 緑（`generic_ops.rs` を新設。20 テスト、うち 1 つは下の「次にやること 4」を記録する `#[ignore]`）

**次にやること**

1. **`errcheck` / `unify` に効く「インターフェースメソッドにレシーバが繋がっていない」**（§7）。
   11 本目の「次にやること 2」（controller-runtime を `pr` に上げる）の残り 8 件のうち
   5 件がこれで、`cases/errcheck-verbose` の ratchet 1 件も同じ根。
   §7 の記録どおり `subst` / `unify` / メソッド集合に波及するので**セッションの頭で**やること。
2. 11 本目からの持ち越し: `manager.Manager has no field or method GetCache` 系の
   ill-typed 16 件（最小再現が取れておらず、`compat/reduce.py`（Phase 6）が先）。
3. 10 本目からの持ち越し: config の validate、gosec G304 / G407。
   6 本目からの持ち越し: §5 の台帳の残り 6 件、SA9008 / SA5011 の σ、govet の未実装 16 pass。
4. `allX` を入れて**まだ**残っている型集合の穴: `range` 文と送受信は
   `commonUnder` を引く**別系統**で、guff 側は `Underlying` 1 段のままである
   （`stmt.rs` の `range_key_val` と `chan_elem` が自分でそう書いている）。
   **測った**: `for range xs`（`T ~[]int`）は `cannot range over xs`、
   map / chan も同様に落ちる。`generic_ops.rs` に `#[ignore]` 付きで
   4 形を置いた（`coverage.py` の `#[ignore]` 走査に乗る）。
   `allX` と同じく**パッケージ丸ごと ill-typed** になる側なので、優先度は 1 と同格。

---

### 2026-08-12（13 本目）— Phase 6: 縮小器とファジングを作り、9 バグを出した

**やったこと**

Phase 6 の道具を両方作った。`compat/reduce.py`（delta debugging）、`compat/fuzz.py`
（差分ファジング）、両者が使う `compat/gospans`（go/ast、stdlib のみ、`oracles/` と同じ作法）。
設計上の要点は §2 の Phase 6 に書いた。**どちらも CI ゲートではない** —— ゲートは「劣化を止める」
道具で、この 2 つは「まだ知らない差分を見つける」道具である。

#### 0. 12 本目の「次にやること 2」は、縮小器が無いから止まっていた

`manager.Manager has no field or method GetCache` 系の ill-typed 16 パッケージが
「最小再現が取れておらず `compat/reduce.py` が先」で 2 セッション寝ていた。
縮小器を作って最初に食わせたのがこれで、実測は **2.6 MB・349 ファイル → 2 KB・2 ファイル /
オラクル 775 回 / 107 秒**。人間が読む対象が 700 ファイルから 20 行になる。

#### 1. ジェネリック実体のメソッドが、型アサーションの経路で展開されていなかった `[縮小器]`

縮小後に残ったのは 20 行で、標準ライブラリすら要らなかった:

```go
type I[T comparable] interface{ add(item T, priority int) }
type S[T any] struct{}
func (S[T]) add(item T, priority int) {}
func New[T comparable]() {
    var m I[T]
    if _, ok := m.(S[T]); !ok { panic("no") }   // guff: impossible type assertion
}
```

guff のエラー文が `queueMetrics[T] does not implement queueMetrics[T]`
（**同じ型が同じ型を implement していない**）と読めるのが手がかりだった。
`have` も `want` も `func(item T, priority int)` で、字面が同じで同一でない
＝ 2 つの `T` が別の TypeParam である。

原因は guff がメソッド集合を**遅延ではなく呼び出し側で**展開すること。Go は
`Named.Method(i)` が遅延展開なので誰が最初に触っても置換済みが返るが、guff は
`expand_instance_methods` を明示的に呼んだ経路だけが正しい。`assignable_to` は呼んでおり、
`assertable_to` → `has_all_methods` は呼んでいなかった。したがって
**「先に代入がある」と隠れる**（`var m I[T] = S[T]{}` を挟むと再現しない）——
遅延完了バグの典型的な指紋である。`prepare_method_set` を
`missing_method` / `implements` / `has_all_methods` の 3 つの入口に置いた。
順序は「解決 → 展開」で固定してある（`expand_instance_methods` は 2 回目を拒むので、
origin のシグネチャが未解決のまま展開すると未解決のものが**恒久的に**焼き付く）。

controller-runtime の ill_typed は **16 → 15**。

#### 2. `//nolint` がファイルを跨いで効いていた `[ファザー / 最初の 1 件]`

`cases/errcheck-asserts` の `defaults/bad.go:9` に `//nolint` を足したら、
**`assert/bad.go:9` の finding が消えた**。

`NolintIndex` の鍵は絶対パス、issue が持つのは**モジュール相対パス**。したがって
`resolve_key` の「basename が一致する最初の絶対パスを返す」フォールバックは
例外経路ではなく**主経路**だった。Go のツリーは `doc.go` / `main.go` / `types.go` で
埋まっているので、衝突は日常的に起きる。しかも症状は「消える」側なので、
**出力にも stderr にも痕跡が残らない**。

相対パス全体を接尾辞として一意に一致させる形に直し、曖昧なら**抑止しない**
（抑止し損ねはユーザに見えるが、他ファイルの finding を消すのは誰にも見えない）。

**そしてこの修正が gin を落とした。** OSS tier が
`gin_test.go:49 gofumpt` の 1 件を extra として捕まえた ——
フォーマッタは**`./` 付きのパス**（`./a.go`）でフィルタに来る（`./` が落ちるのは出力の直前で、
フィルタより後）。接尾辞を生パスから作ると `/./a.go` を探すことになり、
**そのファイルの抑止が全部死ぬ**。旧コードは basename まで削っていたので偶然無事だった。
`./` を剥がしてから引くようにして両方緑にした。教訓は 2 つある:

- **「入口のパス表現」は 1 つではない** —— 絶対・モジュール相対・`./` 付き・basename の
  4 通りが同じ関数に来る。今回はそのうち 1 つしか見ていなかった。
- **ゴールデンでは捕まらなかった。** golden の 81 ケースは全部緑のままで、
  OSS tier だけが落ちた。ケースが `//nolint` とフォーマッタを同時に持っていないからで、
  **実リポジトリを回すゲートを残しておく理由がこれである。**

#### 3. govet `assign` / `shift` —— 上流の 4 条件のうち 3 つが無かった `[ファザー]`

`x = (x)` を自己代入と報告したのが入口。上流を読むと guff に無い条件が 3 つあった:

| 上流の条件 | guff | 症状 |
|---|---|---|
| `typesinternal.NoEffects(lhs)` / `(rhs)` | 無し | `a[f()] = a[f()]` を報告（消すと呼び出しが 2 つ消える） |
| `reflect.TypeOf(lhs) == reflect.TypeOf(rhs)` | 無し | `x = (x)` を報告（後段の比較は go/printer 越しで括弧が消える） |
| 名前は `analysisutil.Format(lhs)` | `id.name`、非 ident は `"_"` | `s.f = s.f` が `self-assignment of _` |

同じ「描画」の欠陥が `shift` にもあり、そちらは**リテラル `"x"`** を使っていた ——
`s.f << 10` も `a[0] << 10` も `(i) << 10` も全部「x (8 bits) too small」。
`printf` が同じ罠を踏んで `describe_arg` で**局所的に**直していたので、
共有ヘルパ `govet_util::format_expr` に引き上げた。この 6 件は 1 つの probe で全部出る。

#### 4. 括弧の方針は linter ごとに違う —— しかも**両方向**に間違えていた `[ファザー]`

`paren` 変異が出した差分を追うと、guff は**片方で剥がしすぎ、もう片方で剥がさなすぎ**だった。

| 上流 | 方針 | guff の誤り |
|---|---|---|
| revive（`errorf` / `range-val-address`） | `exp.(*ast.CallExpr)` 等の**素の型アサーション**＝ 決して剥がさない | 10 箇所で `unparen` していた → `errors.New((fmt.Sprintf(…)))` と `out = (append(out, &v))` で誤検出 |
| honnef の `pattern.match`（S1008） | `case *ast.ParenExpr: return match(m, l.X, r)` ＝ **両側で常に剥がす** | 剥がしていなかった → `return (true)` で黙り、`if (b == true)` の描画に括弧が残る |
| honnef の `astutil.Equal`（QF1003） | `reflect.TypeOf` で始まる＝**剥がさない** | 剥がしていた → `else if (x) == 3` を tagged switch の連鎖に数えた |

**同じ上流プロジェクトの中でも matcher ごとに違う**（honnef はパターン式では剥がし、
構造比較では剥がさない）。したがって「一般に unparen するのが親切」は成り立たず、
**呼んでいる上流の matcher を毎回読むしかない**。3 箇所とも、
その理由をコード中のコメントに残した（「robustness のために足し戻すと誤検出が戻る」）。

#### 5. 「解析 AST にコメントが無い」の **9 例目**、そして初めて人間以外が見つけた `[ファザー]`

`comment` 変異と `nolint` 変異（どちらも結局コメントの挿入）で S1008 が誤検出になった。
上流 S1008 は `ast.NewCommentMap` を張って
「どちらかの枝にコメントがあれば報告しない」を持つ。**guff はそのガードを移植済みだった**
—— `file.comments` が常に空なので、常に「無い」と答えていただけである。

§4 はこの根本原因を buildtag / directive / comments-density / comment-spacings ほかで
**8 回、別々のバグとして**診断している。9 例目。既存の作法どおり `PARSE_COMMENTS` で
再パースし、位置を `pass.fset()` に写像した。写像が要るのは、コメントマップが
**張られた木のノード同一性**で引くから —— 解析 AST の上に張らないと `filter` が何も見つけない。

#### 6. ファザーが副産物で見つけたもの: **golden fixture が Go に受理されない**

`compat/golden/cases/revive` は `go build` が 4 件のエラーで落ちる
（`import "os"` が 2 回、未使用 import 2 件）。これは**互換性のバグではない** ——
golangci-lint も 288 件の revive findings を出すだけで typecheck エラーを 1 件も出さず、
guff も ill-typed と言わない。**2 ツールは一致している。**

が、§7 が「実 Go ツールチェインに一度も読ませていない fixture は、こうなる」と書いた
条件そのものであり、実害として **99 rule / 288 findings を持つ最も濃い fixture を
ファザーが 1 回も回せない**。次にやること 4。

他の 3 件（`staticcheck-sa` / `-s` / `-go114`）はファザー側の欠陥だった:
`go build ./...` は `package main` を**リンク**するので、`func main` を書いていない
fixture が「ビルドできない」と誤判定されていた。リンクエラーだけの失敗は
型検査が通っている証拠なので通すようにした（確認: `--case staticcheck-go114 -n 5` が
`rejected-by-build 0` で 5 ミュータント回る。以前は 100% スキップ）。
これで `staticcheck-sa` の 160 check が `--allow-dirty-seeds` の射程に入る。

**ゲート**

- `cargo test --workspace` — **3065 passed / 0 failed**（回帰テストを 3 本追加:
  `generic_ops.rs` の `type_assertion_sees_instantiated_method_set`、
  `nolint.rs` の `nolint_does_not_leak_to_a_same_named_file_in_another_dir` と
  `dot_slash_prefixed_issue_path_still_resolves`）
- `python3 -m unittest discover -s compat/tests` — **75 passed**（`test_reduce.py` を新設。
  縮小器の編集代数と 2 つの探索ループ —— ここが狂うと**再現しない再現手順**が出る）
- `./compat/filesets.sh --tier pr,nightly` — 8 ターゲット一致
- `./corpus/shapes.py check --offline` — 必須 10 形
- `./compat/golden/run.sh` — **81 ケース**緑（`cases/parens` を新設。ratchet 据え置き: `staticcheck-sa` missing 5 / extra 7、
  `staticcheck-s` missing 2、`staticcheck-st` missing 10、`staticcheck-qf` missing 1、
  `revive` missing 1 / extra 4、`errcheck-verbose` 1/1）
- `./compat/run.sh --isolate` — 116 ターゲット緑
- `./compat/run.sh --oss --tier pr,nightly` — 10 ターゲット緑
  （k9s は 5 回中 1 回だけ goconst で 7 件ずれた。上記 2c）
  （**1 周目は gin が赤**だった。上の 2 を参照）
- `compat/fuzz.py` の 36 件の不一致 — **36/36 が解消**（同じミュータントを再実行して確認）

**次にやること**

1. **残りの ill-typed クラス**。controller-runtime は 16 → 15 になっただけで、
   本丸の `manager.Manager`（埋め込み `cluster.Cluster` のメソッドが 1 つも見えない、63 件）と
   `cannot infer type arguments in call`（24 件）が残っている。前者には**単独の手がかり**がある:
   **`./pkg/metrics/filters/...` だけを解析すると再現せず、`./pkg/...` だと再現する**
   （決定的。3 回とも同数）。つまり**ルート集合に何が入っているかで型検査結果が変わる**。
   縮小は 2.5 時間走らせて 349 → 155 ファイルまで来たところで**打ち切った**
   （ゲートに CPU を回すため。結果は保存していない）。`./pkg/...` がオラクルなので
   1 回 1.5 秒 × ddmin の試行回数がそのまま効く。再開コマンド:
   ```
   python3 compat/reduce.py --dir corpus/cache/controller-runtime \
     --config corpus/cache/controller-runtime/.golangci.yml \
     --packages ./pkg/... --build-cmd 'go vet ./pkg/...' \
     --guff-stderr 'has no field or method Get' -o /tmp/reduced-mgr
   ```
2. **ファザーを回し続ける**。今回は 1 seed・1 変異/ミュータント・12 ミュータント/ケースの
   1 周だけである。`--seed` を変える、`--mutations 2` にする、`--allow-dirty-seeds` で
   ratchet 付き seed（staticcheck-sa の 160 check を含む）に当てる、のどれもまだ空白。
   CI に載せるかは、1 周 852 秒（うち golangci-lint が 647 秒）をどう扱うか次第。
3. **変異の追加**: 識別子リネーム、型の明示/省略、ループ形式変換。どれも型情報が要るので
   `gospans` を go/types 込みにするか、Rust 側に置くかの判断が先。
4. **`cases/revive` を Go が受理する形に割る**（上の 6）。意図的に壊してある宣言
   （二重 import・未使用 import）を専用パッケージに隔離すれば、残りをファジングできる。
5. 12 本目からの持ち越しがそのまま残っている: インターフェースメソッドのレシーバ（§7）、
   `range` / 送受信の `commonUnder`、config の validate、gosec G304 / G407、
   §5 の台帳の残り 6 件、SA9008 / SA5011 の σ、govet の未実装 16 pass。

### 2026-08-13（14 本目）— Phase 6 を閉じ、Phase 7 を作り、controller-runtime の ill-typed を 16 → 0 にした

**やったこと**

Phase 6 の残り 4 項目と Phase 7 を全部。行きがけに、この計画が前提にしていたものが
1 つ崩れた（下記 1）。

#### 1. **上流は入力の関数ではない**

`cases/revive` の fixture を `go build` が通る形にして（下記 2）ゴールデンを再生成したら、
**288 キーのところに 63 キーが書かれた**。レビューに回っていれば
「もっともらしい差分」として通っていた。

golangci-lint 2.12.2 を同じ入力で 24 回回すと、**7 回が 57〜287 件を返す**（正解は 288）。
落ちるのは**パッケージ丸ごと**、毎回違う部分集合、stderr は空、JSON にもエラーは無い。
他の 80 ケースは安定。`--concurrency 1` でも直らない。**暖まった `GOLANGCI_LINT_CACHE`
では安定するが、それを埋めた実行が悪い側だったなら安定して間違っている。**

原因は上流で、しかも**すでに `cases/revive/ratchet.json` が 4 件の恒久差分の犯人として
名指ししている defect の裏面**だった。revive は
`types.Config{Importer: importer.Default()}` で型検査する ——
現代の toolchain に `.a` は無いので、**何かを import しているパッケージは全部**
`Package.TypeCheck()` が失敗する。そして**結果は memo するがエラーは memo しない**
（`lint/package.go`: `alreadyTypeChecked := p.typesInfo != nil`）ので、
**最初の呼び手だけ**が失敗を見る。その失敗を internal failure に変える rule が 2 つあり
（`epoch-naming` / `inefficient-map-lookup`）、`File.lint` は internal failure を見た瞬間に
**そのファイルの残り全 rule を捨てて return する**。ファイルは `errgroup` で並列に
lint されるので、**どのファイルがエラーを引くか＝どの finding が生き残るか**がレースになる。

予測が 2 つとも当たった: **import を 1 つも持たないパッケージは 1 度も finding を落とさない**
（`dot` / `badalias` / `fixtures/` / `footest` / `funclen` / `siblingok` / `sortableok` /
`privatereceiverok`）。そしてその 2 rule を config から外すと 16/16 で安定する。

ratchet が見ていたのはこの defect の**静かな半分**（型が全部 invalid なので rule が黙る）で、
**うるさい半分は「実行ごと結果が変わる」**だった。

対処はハーネス側:

- `golden.py write` が**複数の dump を取り、2 回一致するまで書かない**。
  `run.sh --regen` は最大 8 回回して一致を探す。
- 確認は「同じ答えを 2 回見る」であって「N 回の和集合」ではない。
  和集合は**失われる方向にしか壊れない**ことを仮定するが、revive の
  `package-naming` の memo は finding を**ファイル間で移動させる**別のレースで、
  和集合は両方の位置を「期待値」として焼き付ける。
- `compat/fuzz.py` は逆側から同じ規則を当てる: **不一致を出したミュータントは
  報告する前にもう一度回す**。ミュータントは直後に捨てられるので、
  未確認の報告は**誰も再現できない報告**である。

#### 2. `cases/revive` を Go が受理する形に割った（Phase 6 の「次にやること 4」）

81 ケース中これだけが `go build` を通らなかった。素の 2 回目の `import "os"` と
未使用の `BadAlias` で、Phase 6 の道具は両方とも「Go ツールチェインが受理し続けること」を
不変条件にしているので、**99 rule / 288 findings という最も濃い fixture が
縮小器にもファザーにも触れない**状態だった。

revive の `duplicated-imports` は import **パス**だけで鍵を作る（alias を見ない）ので、
`import osdup "os"` でも重複として報告され、しかも Go が受理する。
上流の finding 数は動かない（288 → 288）。

そしてこの 1 文字が、**古い形では表現できなかった guff のバグ**を即座に出した:
`duplicated-imports` は**パスの列**を報告していた。上流は ImportSpec（`Node: imp`）で、
alias があればその位置になる。alias の無い import では両者は同じトークンなので、
**「alias 付きの重複が fixture に存在できない」限り正しく見えていた**。

#### 3. 2 周目のファジング — 4 件、全部「括弧が答えを決める」場所（Phase 6 の 2）

seed 1・2 編集/ミュータント・**888 ミュータント**で 4 件、全部確認済み・全部修正:

| linter | 形 | 上流 |
|---|---|---|
| errorlint | `err != (nil)` が黙っていた | `isNil` は素の `ex.(*ast.Ident)`。括弧付き nil は nil 比較では**ない**ので報告される。3 つの呼び出し箇所が同じ関数を共有 |
| gocritic `newDeref` | `*new((int))` を `*new(int)` と描画 | 警告は `expr`（書かれたままの StarExpr）に、提案は `astutil.Unparen(call.Args[0])` から。**別のノード 2 つ** |
| SA1006 | `fmt.Printf((s))` が黙っていた | `m.State["format"]` を CallExpr/Ident で type-switch するので括弧付きは飛ばすように読める。飛ばさない —— `pattern.match` が**束縛の前に**両側の ParenExpr を剥がす |
| nolintlint | 使われていない `//nolint` を報告（上流は黙る） | nolintlint は**候補**をディレクティブごとに出し、filter が使われた分を打ち消す。打ち消しは**全 issue と同じ range ループ**を通るので、**その行を覆う任意の range**が、自分が何かを抑止していれば候補を道連れにする |

最後のがファジングの存在理由を 1 段落で説明している。**手書き fixture は range ごとに
ディレクティブが 1 つ**で、1 つなら「**自分の**ディレクティブが何か抑止したか」という
誤読が毎回正しい答えを返す。ファイル頭の `//nolint:errcheck` が実際に errcheck の
finding を 1 件抑止していると、**無関係な下の `//nolint`** が道連れで黙る。
両側から確認した（errcheck の finding を消すと両方が unused として報告される）。

ファザー自身にも穴があった: `issue_key` を直に map していて、golden tier が落とす
`(related information)` 行まで数えていた。`staticcheck-sa` の seed baseline が 5 → 17 に膨らみ、
**ミュータントは baseline を超えたときだけ interesting** なので、
これはノイズではなく**12 件ぶんの目隠し**だった。`--recheck DIR` も足した ——
finding ディレクトリが持っているのはミュータントであってその作り方ではないので、
seed を回し直しても何の証明にもならない。

#### 4. 「型情報が要る」変異 3 種（Phase 6 の 3）

§2 の Phase 6 に書いた。要点は「**不変条件がコンパイルだけなら、変異は正しくなくてよい**」。

#### 5. 縮小する軸はファイルではなかった —— 根集合を縮小したら 64 → 3（Phase 6 の 1）

`manager.Manager has no field or method GetCache` は 2 セッション寝ていて、
前回は 2.5 時間かけて 349 → 155 ファイルまで来たところで打ち切っている。

今回まず**手がかりの方を測った**: `./pkg/metrics/filters/...` では再現せず `./pkg/...` では
再現する（決定的）。つまり**再現条件はファイルの中身ではなく、どのパッケージを
root 集合に入れたか**である。ならば縮小すべきはファイルではなく root 集合で、
そこに ddmin をかけたら **64 → 3 パッケージ**に落ちた（`pkg/cache` + `pkg/manager` +
失敗するテストパッケージ）。オラクル 1 回 1.5 秒、数分。

3 つになれば手で回せる。`{cluster, manager, integration}` は**通る**。
つまり壊れているのは root ではなく**純粋な依存**の `pkg/cluster` の方で、
壊しているのは「その依存 `pkg/cache` が root になったこと」だった。

seed が飲み込んでいる依存側の診断を出す口（`GUFF_DEBUG_SEED_ERRORS=1`）を足して
両方の root 集合を diff したら 2 行だけ差が出た:

```
sigs.k8s.io/controller-runtime/pkg/cluster [.../pkg/cache.test] — 4 errors, first: undefined: intrec
sigs.k8s.io/controller-runtime/pkg/manager                      — 2 errors, first: undefined: cluster
```

**外部テストパッケージの `Deps` は import パスではなく id を持つ。**
`pkg/cache_test [pkg/cache.test]` は `pkg/cluster [pkg/cache.test]` に依存している ——
テストバイナリ用に再コンパイルされた `pkg/cluster` の複製である。
これを import パスとして扱うと、seed は `pkg/cluster` を
**どの `import` 文も綴れない名前**で登録し、`import ".../pkg/cluster"` と書いてある
`pkg/manager` はそれを見つけられない。`cluster.Cluster` が invalid になり、
それを埋め込む `manager.Manager` からメソッドが全部消える。

`dedup::import_path_of_id` で正規化した（seed は production ファイルしかコンパイルしないので、
`Q [P.test]` と `Q` は seed にとって**同じバイト列**であり、潰すのが正しい。
解析にとっては別パッケージなので、そちらでは潰してはいけない）。
`./pkg/...` の `has no field or method` は **48 → 0**、ill-typed は **15 → 7**。

root 集合の ddmin は `compat/reduce.py` の**第 1 パス**として入れた（`--no-reduce-roots` で無効）。
第 2 パス以降のオラクルが 64 パッケージではなく 3 パッケージになるので、
**この 1 パスが後続を全部安くする**。同じ手順を無人で再現することも確認した
（64 → 同じ 3 パッケージ）。

#### 6. 残る 7 は 1 クラス — 型引数推論、そのうち 2 つは別のバグ

全部 `cannot infer type arguments in call`。手で 40 行まで縮めて 2 つに分かれた。

**(a) 推論は引数の method set を読む。** つまり `infer` は
**メソッド集合比較の 4 つ目の入口**であり、13 本目が
`missing_method` / `implements` / `has_all_methods` に置いた `prepare_method_set` を
自分でも呼ばなければならない。同じ遅延完了の継ぎ目の 1 段深いところ。

**同じ型パラメータを名指しする引数が 2 つあるときにしか出ない。**
1 つ目の引数の時点では型パラメータは自由なので、origin 自身の `R` に対して単一化しても
たまたま正しいものが束縛される。2 つ目は `R` が解決済みで到着し、
**置換済みのシグネチャを未置換のものと比較する**。両方とも同じ字面で印字される。

**(b) untyped nil が推論を殺していた。** Go の step-1 の門は `isTyped(arg.typ)` であって
「untyped **定数**か」ではない。untyped `nil` は定数ではなく値なので、
オペランドの mode で判定していた guff はこれを単一化に通し、
型パラメータを含むパラメータ型は untyped nil と単一化しようがないので**推論ごと失敗**した。
`source.Kind(ic, &corev1.Pod{}, nil)` —— `object` は引数 2 に書いてあるのに、引数 3 が沈めた。

controller-runtime の ill-typed は **16（baseline）→ 0**。

#### 7. ill-typed を 0 にすると、**見えていなかった差分が 17 件出てきた**

controller-runtime の weekly tier がこの修正の直後に赤くなった。**recall は 100% のまま**で、
動いたのは precision（**94.6%**）—— 分母が 17 件増えたからである。
`ill_typed` なパッケージは `run_despite_errors` でない analyzer に丸ごと飛ばされるので、
**この 17 件はずっと計算されては捨てられていた**。新しい挙動は 1 つも無い。

§1 が言っていることの縮図である: **見ていないから通っているゲートは、落ちるゲートより悪い。**

内訳と、そこから即座に直った 2 件:

- **nolintlint の「unused」6 件はバグではなく症状。** 上流がその行に
  staticcheck / unparam の finding を出していないのは、**guff が unused と呼んでいる
  まさにそのディレクティブ**で抑止されているからで、guff 側はそもそも撃っていない。
  下の linter を直すと、ここの entry は副作用で消える。
- **実際 3 件がそうやって消えた。** `//nolint:staticcheck` が並んでいたのは
  `clusterOptions.EventBroadcaster = options.EventBroadcaster` のような
  **非推奨の構造体フィールド**への代入で、guff の SA1019 は
  **importer 側のソーススキャンで struct のフィールドを歩いていなかった**
  （関数・メソッド・const/var/type・**インターフェースのメソッド**は歩く。
  最後のは以前のセッションが同じ理由で足したものである）。
  歩かせても直らず、原因はもう一段あった: **フィールドの選択も
  `Info.Selections` に載る**ので `is_method` の判定が
  「selection があるか」では常に真になり、`Options.Old` をメソッド表から探して外していた。
  guff-only は 20 → 17 に減った。
- **`//nolint` が `case` 節の本体を覆っていなかった。** SA1019 を直したら grafana に
  1 件出て、追ったらこれ: Go の `CaseClause.End()` は
  `if n := len(s.Body); n > 0 { return s.Body[n-1].End() }` で、colon が終端なのは
  **空の節だけ**である。guff の `node_end` は常に colon を返していたので、
  節が 1 行のノードになり、`case` の上の `//nolint` は `case` 行しか覆わなかった
  （golangci の range expander は `Node.End()` を読む）。`CommClause`（select）も同文。
  15 行に縮めて確認した。**SA1019 を直さなければ一生見えないバグ**である。
- 残り 17 は `compat/allowlists/controller-runtime.txt` に**理由つきで**記録した。
  SA1019 の逆方向（同一モジュール内の非推奨**型**を guff だけが撃つ）、
  `example_test.go` の SA9003「empty branch」6 件、
  インターフェースを実装するメソッドの unparam 2 件、nilerr と bodyclose が 1 件ずつ。
  **どれも「上流のどのガードが効いているか」を読んでいない**ので、
  allowlist のコメントにそう書いてある。

health baseline も実測値まで下げた: controller-runtime **16 → 0**、
consul 14 → 6、grafana 30 → 21、helm 2 → 1、kubernetes 10 → 8
（後ろの 4 つはこの修正の副作用。ratchet は下げる分には自由だが、
**下げなければ次の劣化を捕まえられない**）。

#### 8. Phase 7

§2 の Phase 7 に設計を書いた。実装は `compat/drift.py` / `compat/pins.json` /
`compat/drift-ledger.json` / `.github/workflows/upstream-drift.yml`。

**ピンが最新なので今日は 0 件で exit 0 する。** それでは道具が動く証拠にならないので、
実在する古いリリース **2.11.4** に当てて検証した。出てきた行は全部説明がつく:
gosec の G124（http.Cookie）と govet の `inline` は 2.11.4 に存在しない、
revive の enable-all 集合が 5 rule 小さい、`clickhouselint` と `gomodguard_v2` が無く
`gomodguard` がまだ deprecated ではない —— **どれも自身の `since: v2.12.0` と一致する**。

**ゲート**

- `cargo test --workspace` — **3078 passed**（回帰テスト 9 本追加: `generic_ops.rs` に推論 4 本、
  `dedup.rs` に id 正規化 2 本、`guff-lint/nolint_test.rs` に打ち消し 1 本、
  `guff-error/checks_test.rs` に括弧付き nil 1 本、`guff-revive/checks_test.rs` に列 1 本 ——
  最後のは列を見る `run_analyzer_at` ヘルパごと。revive の assertion は
  ほとんどメッセージしか見ておらず、**fixture に無い形でだけ列が狂う rule** は
  golden にも単体テストにも映らない）
- `python3 -m unittest discover -s compat/tests` — **96 passed**（`test_drift.py` 新設、
  `test_golden.py` に確認ロジック 6 本）
- `./compat/golden/run.sh` — **81 ケース**緑（ratchet 据え置き）
- `./compat/run.sh --isolate` — 116 ターゲット緑
- `./compat/run.sh --oss --tier pr,nightly` / `--tier weekly` — 13 ターゲット緑。
  **health baseline を実測値まで下げた**（controller-runtime 16 → 0 ほか 4 件）、
  `compat/allowlists/controller-runtime.txt` に 17 件を理由つきで新設（上の 7）
- `./compat/filesets.sh --tier pr,nightly` — 8 ターゲット一致
- `./corpus/shapes.py check --offline` — 必須 10 形
- `compat/fuzz.py --recheck` — 4/4 解消。3 周目（seed 2・新変異込み）は
  **740 ミュータント / 不一致 0 / unconfirmed 1**（その 1 件が上記 1 のレース）

**次にやること**

1. **`--allow-dirty-seeds` がまだ空白。** 3 周目（seed 2・新変異込み・**740 ミュータント**）は
   **不一致 0**、`rejected-by-build` は 1 周目の 4.5% から 3.4% に下がった
   （ヘッダ位置の代入を除外した分）。clean seed 側は当面枯れたと見てよい。
   残っているのは ratchet 付き seed —— staticcheck-sa の 160 check と revive の 99 rule で、
   後者は今回 `go build` が通るようになって初めて射程に入った。

   その 3 周目が確認のしかたも実演している: `revive-enable-all-rules` が 1 件
   `UNSTABLE` を出した（**同じミュータント**の 2 回目が違う差分を出した:
   missing 5 → 3）。上記 1 のレースそのもので、**確認しない設計なら
   「revive の recall バグ 5 件」として報告されていた**。revive を回すときは
   これがミュータントごとに乗る。
2. **他リポの ill-typed を同じやり方で。** 今回の測定で grafana 21 / consul 6 / helm 1 まで
   落ちた（この修正の副作用）が、kubernetes と vault は未測定、grafana の 21 は残っている。
   使う道具はもう `compat/reduce.py` に入っている: **root 集合の ddmin が第 1 パス**で、
   `./pkg/...` の 64 パッケージを 3 に落とすところまでは無人で走る（`--no-reduce-roots` で無効）。
   再現に効く軸がファイルでないなら、ファイルを削っても意味の無い答えしか出ない ——
   **「再現条件は何の関数か」を先に測る**のがこのセッションで一番効いた判断だった。
3. `compat/drift.py` を CI で 1 度も走らせていない（workflow は書いたが schedule 待ち）。
   最初の実走のときに `--update` の出力とレビュー手順を確認すること。
4. **`compat/allowlists/controller-runtime.txt` の 17 件。** ill-typed が 0 になった日に
   見えるようになった precision の穴で、**recall は 100% のまま**である。
   nolintlint の unused は症状なので、下の linter（staticcheck / unparam）を直せば
   まとめて消える。どれも「上流のどのガードが効いているか」を読んでいない。
5. 13 本目からの持ち越しがそのまま残っている: インターフェースメソッドのレシーバ（§7）、
   `range` / 送受信の `commonUnder`、config の validate、gosec G304 / G407、
   §5 の台帳の残り 6 件、SA9008 / SA5011 の σ、govet の未実装 16 pass。

### 2026-08-13（15 本目）— 走査と照合の 2 つの土台が両方ずれていた

**やったこと**

14 本目の「次にやること」5 項目に全部着手した。到達点は項目ごとに違うので、
先に正確に書く:

| 14 本目の項目 | 結果 |
|---|---|
| 1. `--allow-dirty-seeds` | **消化**。staticcheck-sa 220 ミュータント → 4 件（全部 1 クラス、全部修正）、revive 400 → 18 件（4 クラス、全部修正）。残りの dirty seed 4 ケースは未着手 |
| 2. 他リポの ill-typed | vault **0**（初測定）、kubernetes **8 → 1**、副作用で grafana 21 → 14 / consul 6 → 5 / caddy・gin 2 → 1 / helm・k9s 1 → 0。型検査器のバグ 6 種 |
| 3. `drift.py` の初実走 | **完了**。ついでに**レビュー手順に穴**が見つかって塞いだ（下記 9） |
| 4. controller-runtime の allowlist | **17 → 9**。0 ではない。残り 9 のうち unparam 2 + nolintlint 1 は §7 の `MakeInterface` 待ちと**読み終えて**あり、nilerr / bodyclose の 2 件はまだ読んでいない |
| 5. 13 本目からの持ち越し 7 件 | `commonUnder` **解消**、§5 の台帳 **6 件とも決着**、SA5011 の σ **解消**（SA9008 は未着手）、govet は 16 のうち **3 移植 + 1 を §6 へ**。インターフェースメソッドのレシーバ / config の validate / gosec G304・G407 は**未着手**（G304・G407 は見積もりだけ取った） |

行きがけに、**個別のバグではなく道具の側の欠陥**が 2 つ出た（下記 1・2）。
どちらも「1 つ直すと数十のチェックが同時に動く」種類で、
この計画が探しているのはこれである。

#### 1. `preorder` は `ast.Inspect` ではない —— 移植の 4 箇所が走査を打ち切っていた

`fact_deprecated` が **1 つも fact を出していない**ことに気付いたのが入口だった。
SA1019 には「非推奨の関数は非推奨のシンボルを使ってよい」というガードがあり、
その入力は**囲っている関数自身の非推奨性**だけである。`deprs.objects` が空なら
ガードは常に偽で、controller-runtime の 2 件（`WithCustomDefaulter` /
`WithCustomValidator` —— **どちらも自分が `Deprecated:` である**）が出ていた。

原因は 2 つ重なっていた。

**(a) 解析 AST にコメントが無い。** 共有ロードは `PARSE_COMMENTS` 無しで
パースするので `decl.doc` は常に `None`。§4 が buildtag / directive /
comments-density / comment-spacings / S1008 ほかで**9 回**別々に診断してきた
根本原因の **10 例目**。既存の作法どおり再パースして、位置ではなく
**バイトオフセット**で引くようにした（両者は同じバイト列を読むので、
オフセットは写像不要でそのまま一致する）。

**(b) `preorder` の `false` は「部分木を刈る」ではなく「走査ごと止める」。**
guff の `preorder` は Go の `ast.Preorder`（イテレータ、`false` は `break`）の
移植で、ドキュメントにもそう書いてある。ところが**移植元の analyzer は
ほぼ全部 `ast.Inspect`**（`false` は部分木の剪定）で書かれている。
`fact_deprecated` は `import` の GenDecl で `false` を返していたので、
**import を持つ全ファイルで、最初の宣言に着く前に走査が終わっていた**。

`false` を返す 20 箇所を全部読んだ。16 は正しい早期終了で、4 が打ち切りだった:

| 場所 | 上流 | 打ち切りが消していたもの |
|---|---|---|
| `fact_deprecated` | `ast.Inspect` | import 以降の**全宣言**（＝全 fact） |
| `forcetypeassert` | `ast.Inspect` | FuncLit より後ろの型アサーション。ついでに上流は代入を上書きするので**最後の**アサーションを返す（guff は最初のを返していた） |
| `SA9001` | `ast.Inspect` | 最初の return / break / FuncLit より後ろの `defer` 全部。加えて上流の branch 節は `exits = (tok == BREAK)` と**代入**なので、`break` の後の `continue` は `exits` を偽に戻す |
| testifylint `go-require` | `ast.Inspect` | `go` 文より後ろの testify 呼び出し |

`walk::preorder_prune` を足して 4 箇所を移した。`sa4010.rs` は
**この問題に既に出会っていて**、`false` が外側の走査を殺すからと自前のスタックで
回避するコメントを残していた —— 直さずに避けた分、他の 4 箇所は避けなかった。

#### 2. パターンマッチャは**根でしか**括弧を外していなかった

`--allow-dirty-seeds` の 1 周目（staticcheck-sa・220 ミュータント・2 編集）で
4 件の不一致が出て、**4 件とも `paren` 変異**だった。14 本目が SA1006 で、
このセッションが SA6006 で手作業で直した形と同じである。1 つずつ直す前に
`guff-pattern` を読んだら、原因は共通だった:

`match_node` は `unwrap_node_ref` を通すが、フィールドへの下降
（`match_expr_node` ほか）は `match_node_inner` を直接呼ぶ。上流の
`pattern.match` は **`*ast.ParenExpr` を毎回の再帰で、しかも束縛の前に**外す
（`pattern/match.go`）。したがって guff は**パターンの根より下に括弧が 1 つでも
現れた瞬間に黙る**。パターン言語に `ParenExpr` ノードは無い（上流にも無い）ので、
外して困る呼び手は存在しない。`match_node_inner` の入口に括弧剥がしを入れた。

これで SA4024（`(len(s)) < 0`）が直り、手書きの SA1004（`time.Sleep((42))` ——
報告位置も**括弧の中**）と SA1013（`f.Seek((io.SeekStart), 0)`）を同じ規則で直した。
**1 周のファジングが 1 つの構造的欠陥を 3 回別々に指した**ことになる。

#### 2b. revive 側を回したら、**括弧の向きが逆**だった

`staticcheck-sa` を直した後で revive（99 rule / 288 findings）に当てた。
400 ミュータント・2 編集で **18 件**（確認済み）／**86 件が `UNSTABLE`** ——
14 本目が予告したとおり上流のレースがミュータントごとに乗るので、
**確認しない設計なら 86 件の「revive のバグ」が報告されていた**。

生き残った 18 件は 4 クラスで、うち 3 つは同じ結論を指していた:
**revive は括弧を外さない**（照合でも描画でも）。上記 2 の staticcheck とは
**逆向き**で、guff は 3 つの rule で staticcheck 側の作法を使っていた。

| rule | guff | 上流 |
|---|---|---|
| `unnecessary-format` | 括弧を剥がして `fmt.Errorf(("x"))` を報告 | `astutils.IsStringLiteral` は素の `.(*ast.BasicLit)`、表の鍵は `GoFmt(ce.Fun)` の**印字結果**。どちらも括弧を通さない＝黙る |
| `use-fmt-print` | `fmt.Fprintln(os.Stderr, "ok")` と描画 | `astutils.GoFmt` は `go/printer` なので括弧を**残す**: `…, ("ok"))`。`astfmt::expr_fmt` が入口で `unparen` していて `ParenExpr` 節が到達不能だった |
| `redefines-builtin-id` | 名前 Ident の位置（`var len int = 1` で列 6） | `addFailure(n, …)` の `n` は **GenDecl**（列 2）。短い宣言では両者が同じトークンなので、`littype` 変異が `x := 1` を `var x int = 1` に書き換えるまで見えなかった |

4 つ目は別物で、**`var-declaration` の recall 欠落**だった。上流のガードは
1 つだけ（`IsUntypedConst(rhs)` の**既定型**が宣言された型と一致するか）なのに、
guff は上流に対応の無いガードをもう 1 つ持っていた ——
「RHS が untyped const を名指すが `Types` が typed なら飛ばす」。
リテラルの `Types` 項目は**代入文脈で付いた型を必ず持つ**ので、
このガードは全リテラルで発火する。結果 `var a string = "x"` /
`var b int = 1` / `var c float64 = 1.5` / `var d bool = true` が**全部黙り**、
右辺が定数でない場合しか報告していなかった。既定型は `Types` ではなく
**トークン種と定数オブジェクト**から読むように直した（`var e int64 = 1` は
`1` の既定型が `int` なので引き続き黙る）。

#### 2c. k9s の goconst が**時々**7 件ずれる —— 未解明、`run.sh` に確認が無いことの露出

revive の修正後の pr tier で k9s が 1 度だけ落ちた。両側とも 636 件で
**選ばれた定数名だけが 7 件違う**:

```
+guff      container.go:240:goconst:… but such constant `PhaseTerminating` already exists
-golangci  container.go:240:goconst:… but such constant `terminatingPhase` already exists
```

`internal/render` は同じ値の定数を 3 つ持っている
（`PhaseTerminating` pod.go:37 / `Terminating` types.go:13 / `terminatingPhase` pv.go:19）。

**測ったこと**: guff は決定的である（`find_matching_const` が
`(filename, pos)` で整列するので必ず pod.go を選ぶ）。golangci-lint 側も
**直接 9 回**（冷キャッシュ 6・共有キャッシュ 3）回して 9 回とも
`PhaseTerminating` —— **guff と同じ**。にもかかわらずハーネス経由では
`terminatingPhase` が出た。**再現条件は分かっていない。**

分かっていないことより重要なのは、それが**どのゲートにも捕まらない**ことである:
`golden.py` は「同じ答えを 2 回見るまで書かない」、`fuzz.py` は
「不一致は報告前にもう一度回す」という規則を持つ（14 本目、上流のレース対応）。
**`run.sh` にはそれが無い。** 14 本目が revive で見つけたのと同じ形の問題を
OSS tier は 1 回の実行で判定しており、今回はたまたま赤で目に見えたが、
逆向き（本物の差分を 1 回の幸運で見逃す）は静かに起きる。

#### 3. ill-typed: kubernetes 8 → 1、vault は 0（初測定）

vault は「未測定」だったが**実測 0**。kubernetes は 8 パッケージ・44 エラーから
1 パッケージ・2 エラーに落ちた。内訳は型検査器の欠陥 6 種で、
どれも「go build が通るコードで guff だけが落ちる」＝ Phase 1 が数えている側:

1. **untyped な「値」の代入可能性。** `bool(v != 0)` —— go/types の
   `assignableTo` は untyped オペランドを**全部** `implicitTypeAndValue` に渡し、
   その非定数枝は種別だけを見る（`UntypedBool` → `isBoolean(T)`）。
   guff の `representable_closure` は**定数と nil しか**扱わず、比較の結果は
   定数ではないので偽を返していた。これ 1 行で kubernetes の 9 エラー ——
   `generated.pb.go` の unmarshaller が必ず書く形である。
2. **埋め込みを辿らないメソッド署名の遅延解決。** `ensure_method_sigs` は
   自分の名前付き型のメソッドしか `obj_decl` しない。昇格メソッドは未解決のまま
   なので、`var _ I = &Wrap{}` を**メソッド宣言より上に**書くと落ちる。
   `compat/reduce.py` が 483 ファイル → 3 に落とし、手で 12 行になった。
   **順序に依存する = 規則の欠落ではなく遅延完了の欠落**という形。
3. **`_TypeSet.IsComparable` がフラグ読みだった。** `comparable` フラグは
   「`comparable` を明示的に埋め込んだ」しか記録しない。項があるときは
   **項ごとに計算する**のが上流で、`cmp.Ordered` はそれで `comparable` を満たす。
   `Set[T comparable]` を `T cmp.Ordered` で実体化する `util/sets` と
   `api/validate` が丸ごと落ちていた。
4. **`commonUnder`**（12 本目の `#[ignore]`、5 の持ち越し）。`range` と
   チャネル送受信が `Underlying()` を読んでいたので型パラメータで落ちる。
   `#[ignore]` を外した。
5. **`convertible_to` がメソッド集合を準備していなかった。**
   `var _ = net.RoundTripperWrapper(&Transport{})` を、それを満たすメソッドの
   **1 行上**に書く形（kubernetes の `util/proxy` のコンパイル時表明）。
   変換の最初の問いは代入可能性なので、`assignable_to` と同じ遅延完了が要る。
6. **逆方向の型推論（go1.21）。** 引数自身が未実体化のジェネリック関数のとき、
   その型パラメータを呼び先のものと**まとめて 1 回の `infer` に渡す**のが上流
   （`Checker.arguments`）。推論後は各引数を実体化してオペランドの型を差し替える。
   `each([]S{…}, SemanticDeepEqual)` の形で、`api/validate` の 1 ファイルに 21 件。

残る 1 パッケージは `sets.KeySet[string](m)` ——
**明示的な型引数が部分的**な場合（`got < want`）で、`func_inst` がその場で
エラーにしている。埋めるには部分 targs を `infer_call` まで通す必要がある。

副作用で grafana 21 → 14、consul 6 → 5、caddy / gin 2 → 1、helm / k9s 1 → 0。
baseline は全部実測値に下げた。

#### 4. ill-typed が下がると、また**見えていなかった差分**が出てくる

14 本目と同じことが起きた。**recall は 100% のまま**で、動いたのは分母である。

- **caddy に `unused` 2 件。** `iface_method_names` を
  `type X interface {…}` の宣言からしか集めていなかった。
  `wrec.(interface{ setReadSize(*int) })` —— **パッケージ内に閉じたメソッドを
  持つときの定石**がまさに無名インターフェースなので、そこだけ見えていなかった。
  ファイル全体の `InterfaceType` を集めるようにして解消。
- **consul に SA5011 1 件。** これは σ ノードの**残り半分**だった（下記 5）。

#### 5. SA5011 の σ、もう一方の向き —— 既存の allowlist も 1 件消えた

`sigma_shadows` は「先に検査、後で参照外し」を模していた。逆向き ——
SA5011 の doc が宣伝している `_ = *x; if x == nil {…}` の形 —— は
**間に分岐が無い限り**しか上流も報告しない。分岐を 1 つ挟むと、
フォールスルー先は分岐が唯一の先行なので σ が置かれ、下の `if x == nil` は
**σ を比較する**。20 行に縮めて確認した:

```go
v, err := sub(g.SDS)     // g の参照外し
if err != nil { return } // ← ここで g に σ が入る
if g != nil { … }        // σ を比較する: 上流は黙る
```

`renamed_before_check` を足した。consul の新規 1 件と、
**2026-08-09 から allowlist に載っていた `catalog_endpoint.go:280` が同時に消えた**
（あちらは `ns == nil || subj.ChangesNode(ns.Node)` の短絡で、同じ規則）。
consul の明示 allowlist は 3 件 → **2 件**（SA9008 のみ）になった。

#### 6. §5 の台帳を**推測ではなく測った**

「未調査」と「まだ必要」は別の主張で、行を正当化するのは後者だけである。
測り方は 1 つ: **既存の実行の finding 集合を、正規化を 1 つずつ切って鍵付け直す**。

| 行 | 切ったときに増える差分 | 判定 |
|---|---:|---|
| #2 unused の prefix / メソッド修飾 | 12 | **バグを隠していた** |
| #3 staticcheck のコード剥がし | 1 | #4 に帰着 |
| #4 QF1011 / ST1023 の言い回し | 1 | 実在の差（コードも文言も違う 2 チェックが同じ行を撃つ） |
| #5 Deprecated 末尾ピリオド | 0 | **死んでいた** |
| #6 modernize のチェック名 prefix | 2 | **バグを隠していた** |
| #7a govet の pass 名 prefix | 0 | **死んでいた** |
| #7b Go のパッチバージョン | 22 | 環境差（意図的、据え置き） |

- **unused**: guff は honnef が名前の前に置く種別語（`func` / `var` / `const` /
  `type`、`lintcmd/lint.go` の `"%s %s is unused"`）を出しておらず、
  値レシーバを `(T).M` と括弧で包んでいた（上流は `*` を出したときだけ包む）。両方修正。
- **modernize**: 25 チェックのうち 2 つだけが `Diagnostic::category` を設定し、
  残りは空だった。中央で押すようにした —— gocritic の sweep が
  checker prefix を `report()` に移したのと同じ理由で、
  **新しいチェックは書かないものを忘れられない**。

正規化を 4 つ削除した。残っているものは「測って、まだ効いている」ものだけである。

#### 7. govet の「未実装 16 pass」を確定させ、3 つ移植・1 つは §6 へ

台帳から正確な 16 件を出した（golangci-lint 2.12.2 は 46、guff は 30）。
うち**既定で有効なのは 6 つ**で、`enable-all` でしか走らない 10 とは価値が違う。

- **`appends` / `waitgroup` / `hostport` を移植**し、`cases/govet` に載せた
  （82 キー・完全一致）。`waitgroup` は SA2000 と**同じ問いで別の答え**を出す:
  上流は固定のスタック形（`GoStmt / CallExpr / FuncLit / BlockStmt / ExprStmt /
  CallExpr`）に加えて **`ExprStmt` がブロックの最初の文であること**を要求し、
  `(` の位置に固定文言を報告する。SA2000 は本体を再帰的に探して式を描画する。
- **`asmdecl` は §6 行き（実測）。** `go vet` は
  `a_arm64.s:4:1: [arm64] Add: wrong argument size 8` を出すが
  golangci-lint 2.12.2 は **0 件**。`framepointer` と同じで、
  **golangci-lint は `.s` の診断を 1 件も出さない**。
  ゲートで観測する方法が原理的に無い。

残り 12（既定で有効な `stdversion` / `testinggoroutine` と、`enable-all` 専用の 10）。

#### 8. controller-runtime の allowlist 17 → 9

- **SA9003 の 6 件は `irutil.IsExample` だった。** 上流は `SrcFuncs` を回す
  ループの**最初**にこれを訊く（`fn.Source()` を見るより前）。
  `Example` で始まる名前 + `_test.go` の 2 条件だけで、無名関数は
  `ExampleFoo$1` なので prefix で一緒に入る。SA4006 も同じガードを持つので
  そちらにも入れた。
- **SA1019 の 2 件は上記 1 の fact 欠落**。両方とも自分が非推奨のメソッドだった。
- 残り 9 のうち **unparam 2 件は読み終わった**: 上流は
  `ssa.MakeInterface` から `typesImplementing` を作り
  （`addImplementing(findNamed(instr.X.Type()), iface)`）、その名前のメソッドを
  飛ばす。guff-ssa の `MakeInterface` は `pub struct MakeInterface {}` で
  **箱詰めする値も型も持たない** —— §7 が SA4006 について記録しているのと
  同じ欠落で、SSA を 1 つ直すと unparam 2 件と
  `//nolint:unparam` の症状 1 件が同時に消える。
- nilerr / bodyclose の各 1 件は**まだ読んでいない**（そう書いてある）。

#### 9. Phase 7 の初実走 —— レビュー手順に穴が空いていた

`compat/drift.py` はピンが最新なので 0 件 exit 0。それでは `--update` の経路が
通らないので、14 本目と同じく実在の旧版 **2.11.4** に当てて動かした。
台帳は書けた（gosec / govet / inventory の 3 エントリ、`why` は全部 placeholder）。

**そこで台帳をそのままにして再実行したら、exit 0 で「No unreviewed drift」だった。**

workflow のコメントは「commit する前に `why` を全部埋めること」と書いてあるが、
**強制するものが何も無かった**。`--update` の出力をそのまま commit すれば
週次ジョブは黙り、記録されているのは `TODO: …` の 3 行である。
§1 が言っている「見ていないから通っているゲート」の、**ジョブ自身での再演**だった。

`is_reviewed` を足して、空・`TODO` 始まり・placeholder のままの `why` を
「レビュー済み」と認めないようにした（inventory 側も同じ）。埋めた `why` で
再実行すると exit 0 に戻ることも確認してある。
検証用に書いた 2.11.4 の台帳は**消した** —— ピンより古い候補をレビューした記録は
次のバンプについて何も言わないし、`ledger_verdict` は候補が変われば
どのみち全部を未レビューに戻す。

**ゲート**

- `cargo test --workspace` — **3087 passed / 0 failed**
  （12 本目の `#[ignore]`（`commonUnder`）を解除、govet に 6 本、revive に 2 本追加）
- `python3 -m unittest discover -s compat/tests` — **98 passed**
  （`test_normalize.py` の 4 本は「正規化しないこと」を主張する側に書き換え、
  `test_drift.py` に placeholder な `why` を拒む 2 本）
- `./compat/golden/run.sh` — **81 ケース**緑、ratchet 据え置き
  （`cases/govet` は 80 → 82 キー。**revive の 4 クラスを直しても ratchet は
  1/4 のまま** —— seed は変異していないので当然だが、
  「seed で見えない形」を直したという証拠でもある）
- `./compat/run.sh --isolate` — 116 ターゲット緑（**正規化を 4 つ外したまま**）
- `./compat/run.sh --oss --tier pr,nightly` — 10 ターゲット緑
  （k9s は 5 回中 1 回だけ goconst で 7 件ずれた。上記 2c）
- `./compat/run.sh --oss --tier weekly` — 5 ターゲット緑
- health baseline を全部実測値に: kubernetes 8 → 1、grafana 21 → 14、
  consul 6 → 5、caddy / gin 2 → 1、helm / k9s 1 → 0、
  controller-runtime / vault / cobra / containerd は 0 を明示
- `compat/allowlists/` — controller-runtime 17 → 9、consul 3 → 2
- `./compat/coverage.py all` — 550 checks / `fired` 546 / `unit-only` 1 / `never` 3

**次にやること**

1. **dirty seed の残り 4 ケース。** `staticcheck-sa`（220 ミュータント）と
   `revive`（400）は回した。`staticcheck-s` / `-st` / `-qf` /
   `errcheck-verbose` はまだで、ratchet がどれも recall 側（missing）なので
   件数比較の信号は staticcheck-sa より読みやすいはずである。
   revive の 18 件のうち**4 クラスは全部直した**が、残りの `EXTRA`
   （`unchecked-type-assertion` / `unnecessary-if` /
   `inefficient-map-lookup` / `error-strings` / `if-return`、各 1 件）は
   **どれも `paren` 変異が付いている**ので同じ「括弧を外さない」クラスの
   可能性が高い。1 つずつ最小化すること。
2. **`run.sh` にも「2 回一致するまで」を入れる**（上記 2c）。`golden.py` と
   `fuzz.py` は持っていて OSS tier だけが持っていない。k9s の goconst 7 件が
   その不在を偶然に露出させた —— 赤で出たから見えただけで、
   **1 回の幸運で本物の差分を見逃す向き**は今も静かに起きうる。
   ついでに k9s の再現条件も特定すること（guff 側は決定的、
   golangci-lint 単体でも 9 回とも guff と同じ答え、ハーネス経由でだけ違う）。
3. **`MakeInterface` にオペランドを持たせる。** これ 1 つで
   §7 の SA4006（golden の extra 1 件）、controller-runtime の unparam 2 件、
   その `//nolint:unparam` の症状 1 件が同時に消える。SSA の構造変更なので
   セッションの頭でやること。
4. **部分的な明示型引数**（`sets.KeySet[string](m)`）。`func_inst` が
   `got < want` でその場でエラーにしているのを、部分 targs を持ったまま
   `infer_call` に渡す形に変える。kubernetes の最後の 1 パッケージがこれ。
5. **govet の残り 12。** 既定で有効なのは `stdversion` と `testinggoroutine` の
   2 つだけで、そこが先。残り 10 は `enable-all` 専用。
6. **gosec G304 / G407 は「実装が無い」の中でも重い方**だと分かった。
   G304（`rules/readfile.go`、246 行）は gosec の taint / resolve エンジンに
   乗っており、guff はそれを近似で持っている（`gosec.rs` の DEFERRED 参照）。
   G407（`analyzers/hardcoded_nonce.go`、**878 行**）は SSA 走査そのもの。
   どちらも単独の投資として見積もること。
7. **config の validate は 8 規則まで列挙が揃っている**（7・8・9 本目からの持ち越し）。
   一箇所にまとめておく —— 実装先はどれも `config.rs`（`ConfigError`）で、
   7 本目が 68 config を走査して「これを足しても OSS / regress のゲートは動かない」
   ことまで測ってある:
   1. 条件が 2 個未満の除外規則（`severity` 規則は 1 個）
   2. `path` と `path-except` の同時指定
   3. preset 名の検証（guff は `stdErrorHandling` のような camelCase も受ける）
   4. `severity.rules` があるのに `severity.default` が無い
   5. `output.path-mode: rel`
   6. gocritic の `enable-all` + `enabled-tags`
   7. gocritic の `disable-all` + `disabled-checks`
   8. gocritic の `disable-all` だけで何も enable しない
   **golden tier は「上流が起動を拒む」を表現できない**ので、
   ここに列挙が溜まる形は変わらない。実装したら `compat/tests` 側に
   「拒むこと」のテストを置くこと。
8. 持ち越し: インターフェースメソッドのレシーバ（§7 —— `errcheck-verbose` の
   ratchet 1/1 と `build_exclude_set` の別名追加が同時に消える）、
   SA9008 の IR 検証（consul の allowlist 2 件、`cases/staticcheck-sa` の extra）、
   controller-runtime の nilerr / bodyclose 各 1 件（**まだ上流を読んでいない**）。
### 2026-08-13（16 本目）— OSS tier から「1 回の実行で決める」を外し、「上流が起動を拒む」に tier を 1 つ立てた

**やったこと**

15 本目の「次にやること」から 5 項目に着手した。到達点は項目ごとに違う:

| 15 本目の項目 | 結果 |
|---|---|
| 2. `run.sh` にも「2 回一致するまで」 | **消化**。`--confirmations`（既定 2）を入れ、確認できないターゲットは `UNSTABLE` で落とすようにした。k9s の goconst の**再現条件は特定できていない**（ハーネスと同じ形で 18 回、うち 10 回は 8 コア負荷下 —— 全部同じ答え）。分かったことは下記 2 |
| 7. config の validate 8 規則 | **消化 + 2 件**。実装しながら**上流の同じ関数の隣にある 2 規則**（`linters` に formatter を書く／`formatters` に linter を書く）が同じ箱だと分かったので入れた。加えて**新しい tier**（`compat/reject/`）を作った —— 両ツールを走らせて「両方拒む・理由も同じ」を確認する 12 ケース |
| 4. 部分的な明示型引数 | **消化**。kubernetes の ill-typed は **1 → 0**（baseline から行ごと消えた） |
| 5. govet の残り 12 | **1 つ消化**（`testinggoroutine`、golden **90/90** 完全一致）。行きがけに**「`Ident.obj` を読む analyzer」の名簿**という落とし穴を踏んだ（下記 4）。`stdversion` は見積もりだけ取った（下記 5） |
| 1. dirty seed の残り 4 ケース | **1 ケース目（`staticcheck-s`）を消化: 16 件 → 1 件**。下記 7 |

#### 1. `run.sh` は 1 回の実行でターゲットの合否を決めていた

`golden.py` は「同じ答えを 2 回見るまで golden を書かない」、`fuzz.py` は
「不一致は報告前にもう一度回す」。**OSS tier だけがその規則を持っていなかった**。

入れた形は golden 側と同じである: golangci-lint を最大
`--confirm-attempts`（既定 4）回まで走らせ、**正規化キー集合が 2 回一致した実行**を
diff に使う。一致しなければそのターゲットは `UNSTABLE` で**落とす**——
比較対象の答えが 1 つに決まらない日に、その答えのどれかを選んで緑を出すのは
このハーネスがやってはいけないことの側である。

- 確認は**毎回まっさらな `GOLANGCI_LINT_CACHE`** で行う。使い回すと 2 回目は
  1 回目の答えをキャッシュから再生するだけなので、確認が何も確認しなくなる。
- 判定は `normalize.py confirm`（`golden.py` の `confirm` と同じ「N 回見た」規則。
  和集合ではない —— revive のメモ化レースは finding を**移動**させるので、
  和集合は移動前と移動後の両方を正しいことにしてしまう）。
- `--confirmations 1` で従来どおり（速い反復用）。既定は 2。
- **代償は golangci-lint 側の実行が 2 倍**になること。これは「本物の差分を
  1 回の幸運で見逃す」向きを塞ぐのに必要な最低額である: 上流が丸ごと落とした
  パッケージが**たまたま guff も落としている** finding を含んでいたら、
  差分は空になり、ゲートは緑のまま recall バグを隠す。

#### 2. k9s の goconst —— 再現しない、そして「しない」ことの方に意味がある

15 本目が見た「7 件だけ定数名が違う」を再現しようと、**ハーネスと同一の起動**
（同じ patch 済み config・`--path-mode abs`・`--max-*-issues=0`・
`--allow-parallel-runners`・毎回新しいキャッシュ）で 8 回、さらに**8 コアを
占有した負荷下**で 10 回回した。**18 回とも 636 件・`PhaseTerminating` 3 件**で、
15 本目が見た `terminatingPhase` は 1 度も出ていない。

上流を読んだ範囲では、goconst の**選択そのものは決定的**である:

- `sortConstants` → `lessPosition` は**ファイル名 → 行 → 列**の順で比較する
  （`api.go`、v1.10.2）。3 つの定数は `pod.go` / `pv.go` / `types.go` にあるので
  タイエは無く、`sort.Slice` が不安定であることも効かない。
- `p.consts` への追記は `constMutex` の下で、収集は `wg.Wait()` の後。
- `InternString` は `sync.Map` で、内容を変えない。

したがって**「pod.go と types.go の定数がその実行の入力に無かった」以外の説明が
残っていない** —— 上流が仕事を落とす向きの症状で、`golden/README.md` の
「Upstream is not a function」と同じ箱に入る。**再現できていない**と書いておく。
上記 1 の確認規則は、この形が次に出たときに**赤ではなく `UNSTABLE`**として
（＝「上流が答えを 1 つに決められなかった」として）出す。

#### 3. 「上流が起動を拒む config」に tier を作った —— `compat/reject/`

7・8・9 本目が列挙し、15 本目が 1 箇所にまとめた 8 規則を実装した。
実装先は `config.rs` の `ConfigFile::validate`（`ConfigError::Validation`）。
**メッセージは上流と 1 文字も違わない**（`can't load config: …` の接頭辞ごと）:

| 規則 | 上流 |
|---|---|
| 除外規則の条件が 2 個未満 | `BaseRule.Validate(2)` |
| `path` と `path-except` の同時指定 | `BaseRule.Validate` |
| preset 名（kebab-case のみ。`stdErrorHandling` は**拒む**） | `LinterExclusions.Validate` |
| `severity.rules` があって `severity.default` が無い | `Severity.Validate` |
| severity 規則の `severity` 未指定 / 条件 1 個未満 | `SeverityRule.Validate(1)` |
| `output.path-mode: rel` | `Output.validatePathMode` |
| gocritic の 6 通りの組み合わせ | `validateOptionsCombinations` |
| **`linters.enable/disable` に formatter**（+2） | `Linters.validateNoFormatters` |
| **`formatters.enable` に linter**（+2） | `Formatters.Validate` |

**gocritic だけは config ロード時ではない。** 上流はこの検証を linter の
context setter（`logger.Fatalf`）でやるので、**gocritic が有効なときだけ**発火する
—— 無効な linter に古い settings ブロックが残っている config は上流では起動するし、
guff もそうでなければならない。`ConfigError::LinterSettings` を別に立てて、
`guff run` が enable 集合を解決した後に呼ぶ形にした。

**移植しなかったものが 1 つある**: 上流は `path` / `text` / `source` を
**Go の regexp としてコンパイル**して、失敗したら config ごと拒む。guff の照合は
Rust の `regex` で、方言は**両方向に**食い違う（Go が通す先読みを Rust は拒む）ので、
これを写すと**上流が走らせる config を guff が拒む**。写さない理由ごと
`config.rs` と `compat/reject/README.md` に書いた。

**新しい tier**（`compat/reject/run.sh`、12 ケース）は、finding 集合の tier では
**原理的に表現できない**ものを見る:

- golangci-lint が**今も**拒むこと（記録した理由を上流が出さなくなっていたら、
  そのケースは黙って何もテストしなくなっている）
- guff も拒むこと
- **理由が一致**すること（枠は違う: 上流は `Error: …` / `level=error msg="[linters_context] …"`、
  guff は `guff: …`。`reject.py` が枠を外して中身を比べる）

期待値は `--regen` が**上流の実際の出力から**書く（golden と同じ規則。誰も期待値を手で書かない）。
`cases/_control/` は**両ツールが走らなければならない** config で、
「全部落ちているのに緑」を防ぐ。

**このゲートが空振りでないことも確かめた**: 実装前の
`linters.enable: [gofmt]` を 1 ケース足すと、guff は拒みはするが理由が違う
（`linter "gofmt" is not available yet`）と赤で出た。それが上記 +2 の入口である。

**副産物 3 つ**

1. **guff 自身の fixture が 1 つ、上流の起動しない config だった。**
   `tests/testdata/config/v2_full_issues.yml` は v2 なのに v1 の綴り
   `severity.default-severity` を書いていて、golangci-lint 2.12.2 は
   `can't set severity rule option: no default severity defined` で拒む（実測）。
   fixture を v2 の綴りに直し、テストは「v2 のキーが `effective_severity` に載る」
   側を主張するようにした。
2. **リポジトリと corpus の config 1,813 件を新しい validate に通した** ——
   拒まれたのは上の fixture と `compat/reject/cases/` の 11 件だけ。
   R22 の実 config コーパス（52 件）テストに `validate()` を足したので、
   **規則を厳しく写しすぎたら実在のリポジトリの config が赤で出る**。
3. **終了コードは合わせていない**（記録）。上流はこの家族を全部 **3** で終える。
   guff は 2（`docs/COMPATIBILITY.md` が明記している既定）。tier は
   「両方拒む」を主張していて、数字は主張していない。

#### 4. `testinggoroutine` を移植したら、名簿に載っていなかった

`go` 文の中の `t.Fatal` / `Skip` / `FailNow`（＝ `runtime.Goexit` で終わるメソッド）を
撃つ pass。golden ケース `cases/govet` に fixture を 2 本足して回したら
**89/90 一致**で、欠けた 1 件は `fn := func(){ t.FailNow() }; go fn()` の形だった。

原因は analyzer ではなかった。**guff は `Ident.obj`（パーサのスコープ解決）を
読む analyzer が 1 つも有効でないとき、解決自体を飛ばす**（P0-3 の最適化）。
その名簿 `AST_OBJECT_RESOLUTION_ANALYZERS` は `ineffassign` と `maintidx` の
2 つで、上流が `id.Obj` で辿るこの形は**静かに None を引いていた**。
**名簿から漏れた analyzer は落ちない、黙るだけ**である。名簿に足して 90/90。
名簿のコメントに、この症状の形を書いた。

移植の中身で写した細部:

- **region（regions）という構造**。`go fun()` と `t.Run(name, fun)` がそれぞれ
  「並行に走る範囲」を作り、**入れ子になった region の中の呼び出しは内側のものだけ**が
  見る。`-subtest` は golangci-lint では常に off なので `t.Run` region は
  **何も報告しない**が、**集めなければならない** —— goroutine の中の
  subtest リテラルの中の `t.Fatal` を、集めないと goroutine のものとして誤報する。
- 報告位置は `go` 文（ただし起動するのがリテラルなら**その呼び出し自身**）。
- 文言の `(fn calls (*testing.T).FailNow)` は、起動対象が識別子のときだけ付く。
- レシーバ型は**セレクションの受け手**を描く（埋め込み越しなら外側の型が出る）。

#### 5. `stdversion` は「1 パスの移植」ではない（見積もり）

既定で有効な残りはこれだけになった（`asmdecl` は 15 本目が §6 送りにしている）。
中身は 60 行だが、答えは `typesinternal.TooNewStdSymbols` →
`internal/stdlib.PackageSymbols` という**生成された表**に全部入っている
（x/tools の `manifest.go` は **18,613 行**、stdlib の全シンボル × 導入バージョン）。
guff 側に必要なのは:

1. 同じ表の Rust 版と、**その生成器**（`$GOROOT/api/go1.*.txt` か上流 manifest から）
2. **表が Go のリリースに追随している**ことを見るゲート（`compat/oracles` と同じ形の
   pin + regen。表が古いと**黙って撃たなくなる**側の欠陥になる）
3. 生成ファイル抑止と「too-new な型のフィールド/メソッドは撃たない」パス 2

**gosec G304 / G407 と同じ箱**（＝単独の投資として見積もる）に入れる。

#### 6. 部分的な明示型引数 —— kubernetes の最後の 1 パッケージ

`sets.KeySet[string](m)`。上流の `callExpr` は**書かれた分の型引数を持ったまま
signature を generic のままにして** `arguments` に渡し、
`arguments` が引数の型と合わせて**1 回の `infer`** にかける
（`for len(targs) < len(tparams) { append(nil) }`）。guff の `func_inst` は
`got < want` をその場で `CannotInferTypeArgs` にしていた。

上流と同じく `func_inst` に `infer` フラグを足し、**呼び出し位置では
部分 targs を返す**ようにして、`infer_call` の `targs_in` の**先頭に**据える形にした
（位置で対応する: `two[int](1, "s")` は A = int を固定して B を推論する）。
値位置（`_ = two[int]`）は上流と同じくエラーのまま —— 代入先から推論する
`target` の機構は移植していない。

kubernetes の ill-typed は **1 → 0**（`compat/baselines/health.json` から行ごと削除。
不在＝厳格に 0）。

#### 7. dirty seed（`staticcheck-s`）

#### 7. dirty seed の 1 ケース目（`staticcheck-s`）—— 括弧の話、三度目

`--allow-dirty-seeds` で `staticcheck-s`（120 ミュータント / 2 編集 / seed 1）を回して
**16 件**。内訳を数えると 1 つの形に寄っていた: **9 つの check が「括弧が 1 つ入ると黙る」**。

15 本目が直したのは `guff-pattern` の `match_node_inner`（＝パターン言語を通る側）で、
**これらの check はパターンを通っていない**。上流は 9 つとも `pattern.MustParse` の query で
書かれていて、`pattern.match` は再帰のたびに、しかも**束縛の前に** `*ast.ParenExpr` を外す。
guff は手で下降しているので、**下降のたびに `unparen` が要る**。

| check | 上流の query が外す所 | guff が素で見ていた所 |
|---|---|---|
| S1007 / S1039 | `[lit@(BasicLit "STRING" _)]` | 第 1 引数 |
| S1038 | `[(CallExpr (Symbol "fmt.Sprintf") _)]` | 内側の呼び出しと、その第 1 引数 |
| S1035 | `[(CallExpr (Symbol "net/http.CanonicalHeaderKey") …)]` | 引数・`call.fun`、**そして報告位置**（束縛は括弧を外した節なので、上流は `(` の 1 つ内側を指す —— 列が 1 ずれていた） |
| S1009 / S1020 / S1031 / S1033 / S1036 | `BinaryExpr` / `IfStmt` / `AssignStmt` / `IndexExpr` の分解 | 条件・左辺・右辺、および各ファイルの `same_expr` |

**16 件のうち 1 件は括弧ではなかった。** `rename` 変異（`ok` → `err`）が、
S1020 のメッセージ `when ok is true, …` の **`ok` がハードコード**だったことを出した。
上流は `assign.Name()` —— **その場の変数の名前**を出す。
木の中の fixture が全部その変数を `ok` と名付けていたので、
文言は今日まで「たまたま」正しかった。

同じ seed で回し直して **16 → 1**。残る 1 件は `comment,swap` 変異が
`s1021/ok.go` の `if err == nil` を `select` の下へ動かした形の SA4006 recall 落ちで、
**このケースの ratchet（missing 2・SA4006）と同じ側**である
（そこまでは確認した。IR まで追ってはいない）。

**行きがけに 1 つ、変異が触っていない所も出た。** S1036 の `+=` / `++` の枝は
**guff では死んでいた**: 上流の query が同じ束縛 `indexexpr` を 2 度使う所を、
guff は **AST のノード id 比較**（`else_assign.lhs[0].id() == x.id()`）で書いていて、
別々のノードが id を共有することは無いので**必ず偽**だった。
fixture が `append` の形しか持っていなかったので、golden も単体テストも
その枝に一度も触っていない。`same_expr` に直し、fixture に 2 形を足して
golden を再生成すると **golangci-lint も 3 件**（＝ guff と一致）。
`cases/staticcheck-s` は 90 → 92 キー。

**`staticcheck-st` / `-qf` / `errcheck-verbose` は未着手。**

**ゲート**

- `cargo test --workspace` — **3,116 passed / 0 failed**
  （config の validate 21 本、部分 targs 6 本、`testinggoroutine` 2 本を追加）
- `python3 -m unittest discover -s compat/tests` — **118 passed**
  （`test_normalize.py` に確認規則 7 本、`test_reject.py` を新設して 13 本）
- `./compat/golden/run.sh` — **81 ケース**緑。`cases/govet` は 82 → **90 キー**
  （`testinggoroutine` の 8 件）、`cases/staticcheck-s` は 90 → **92 キー**（S1036 の 2 形）。
  ratchet は 4 つとも据え置き
- `./compat/reject/run.sh` — **新設・12 ケース**緑（`_control` 込み）
- `./compat/run.sh --isolate` — 116 ターゲット緑
- `./compat/run.sh --oss --tier pr,nightly` — **10 ターゲット緑**。
  **確認は 1 度も追加実行を要さなかった**（10 ターゲット × 2 回で全部一致。
  `--confirm-attempts` の 3 回目に進んだターゲットは 0）
- health baseline を実測値に: **kubernetes は行ごと削除**（不在＝厳格に 0）、
  consul 5 → 4、grafana 14 → 13（どちらも部分 targs の副作用）
- `./compat/coverage.py all` — **551 checks** / `fired` 547 / `unit-only` 1 / `never` 3
- リポジトリと corpus の **config 1,813 件**を新しい validate に通し、
  拒まれたのは `compat/reject/cases/` の 11 件と、**上流も拒む guff 自身の fixture 1 件**だけ
- `./compat/fuzz.py --case staticcheck-s --allow-dirty-seeds -n 120 --mutations 2 --seed 1`
  — 16 件 → **1 件**（残りは ratchet と同じ SA4006 recall）

- `./regress/run.sh --profile full` — **赤。ただしこのセッションの変更のせいではない**（下記 8）


#### 8. regress の full profile が赤かった —— **15 本目で動いていた**

`testinggoroutine` が `Ident.obj` を要求する（上記 4）ので、**govet が有効なとき
P0-3 の「スコープ解決を飛ばす」が効かなくなる**。govet は既定で有効なので既定の
経路が変わる。だから perf を測った。結果は赤で、しかも**2 軸とも**赤だった:

| | baseline（`regress/baseline.full.json`） | 実測 |
|---|---:|---:|
| wall | 2.360s | **4.150s** |
| guff_only | 0 | **4** |

**まず自分の変更を疑って A/B した**（docs/PERF_TASKS_V2.md §1.2 の作法）。
HEAD（15 本目の `a568aef`）の release バイナリを worktree で建てて、
同じ prometheus に交互に 3 往復:

| round | HEAD | このセッション |
|---|---:|---:|
| 1 | 4.172s | 4.174s |
| 2 | 4.250s | 4.308s |
| 3 | 4.350s | 4.363s |

**差は 1.5% 未満＝雑音**。finding 集合も **HEAD と完全一致**（24 件、同じキー）。
つまり `Ident.obj` の解決を戻した代償は、この計測では見えない。

そこで**どこで動いたか**を二分した（各コミットを建てて 3 回ずつ）:

| コミット | wall | findings |
|---|---:|---:|
| `b96fd2e`（13 本目 / Phase 6） | 2.97 – 2.98s | **20** |
| `6a3cd9a`（14 本目の最後） | 2.98 – 3.07s | **20** |
| `a568aef`（15 本目） | 4.17 – 4.35s | **24** |
| このセッション | 4.17 – 4.36s | 24 |

**15 本目で wall が +40%、guff_only が 0 → 4 になっている。**
baseline が最後にロックされたのは 5 本目（`5705ad7`）で、
**記録に残っている最後の実行は 8 本目**（A/B の中央値 2.490s、§4 の同エントリ）。
9〜15 本目のゲート欄に `regress` は 1 度も出てこない —— §1 が言っている
「見ていないから通っているゲート」が、7 セッション続いていたことになる。
（この 2 つは別の主張である: baseline を再ロックしないのは正しい運用だが、
**回さないのは運用ではない**。）

4 件のうち 2 件は**その場で形が分かった。どちらも `const` グループ**である:

- `config/config.go:579` revive `var-declaration` —— `const (…)` の中の
  `UTF8NamesHeader string = model.EscapingKey + "=" + …`。15 本目は
  「既定型を `Types` ではなくトークン種と定数オブジェクトから読む」と書き直しており、
  上流が黙るこの形で撃つようになっている。
- `tsdb/head.go:240` SA9004 —— `const (…)` で**各定数に doc コメントが付いている**形。
  上流はこのグループを撃たない。

残り 2 件（`tsdb/example_test.go:58` SA4006、`tsdb/querier.go:243` SA5011）は
**まだ読んでいない**。SA4006 の方は 15 本目が `irutil.IsExample` のガードを入れた
まさにそのファイル種別（`example_test.go`）なので、そこが起点である可能性が高い。

**baseline は再ロックしていない。** 76% 遅くなった wall と 4 件の偽陽性を
「新しい正常」として黙らせることになるからで、**赤のまま次のセッションに渡す**。

**次にやること**

0. **regress の full profile を赤のまま引き継いだ（上記 8）。** ここから始めること:
   guff_only 4 件のうち 2 件は形まで分かっている（revive `var-declaration` と
   SA9004 が `const` グループで撃つ）。残り 2 件を読み、4 件とも潰してから
   wall の +40% を追う（**先に finding を直すこと** —— 15 本目の変更が
   ill-typed を減らした結果として仕事が増えている可能性があり、
   それなら wall は「正しく増えた」分と「無駄に増えた」分の合計になる）。
1. **dirty seed の残り**: `staticcheck-st` / `staticcheck-qf` / `errcheck-verbose`。
   `staticcheck-s` の結果（上記 7）を踏まえて。
2. **`MakeInterface` にオペランドを持たせる**（15 本目からの持ち越し、優先度そのまま）。
   §7 の SA4006（golden の extra 1 件）、controller-runtime の unparam 2 件、
   その `//nolint:unparam` の症状 1 件が同時に消える。SSA の構造変更なので
   セッションの頭でやること。
3. **govet の残り 11**。既定で有効なのは `stdversion` だけで、それは上記 5 の
   見積もりどおり単独の投資。残り 10 は `enable-all` 専用
   （atomicalign / deepequalerrors / fieldalignment / findcall / httpmux /
   nilness / reflectvaluecompare / shadow / sortslice / unusedwrite）。
   **`nilness` と `shadow` は SSA / スコープを使う側**で、残りは小さい。
4. **確認の代償を測る/減らす。** golangci-lint 側が 2 倍になったぶん、CI の
   `oss-pr` / `oss-nightly` / `isolate` の timeout を上げてある（90 / 150 / 75 分）。
   2 回の実行は**互いに独立**（別キャッシュ・`--allow-parallel-runners`）なので
   並行に走らせれば壁時計はほぼ元に戻るはずだが、grafana 級のターゲットを 2 つ
   同時に走らせるメモリを測っていない。**測ってから**やること。
5. **`compat/reject/` を広げる**先: 上流の validator でまだ写していないのは
   `Run.Validate`（`modules-download-mode` / `relative-path-mode` の値検証）と
   `Output.validateSortOrder`。どちらも 10 行程度で、`reject` の 1 ケースずつ。
   **終了コード 3 に合わせるかどうか**もここで決めること（上記 3 の副産物 3）。
6. 持ち越し: インターフェースメソッドのレシーバ（§7）、SA9008 の IR 検証、
   controller-runtime の nilerr / bodyclose 各 1 件（**まだ上流を読んでいない**）、
   gosec G304 / G407。

### 2026-08-13（17 本目）— 赤かった 2 軸を分けて畳んだ: finding は 0 に、wall は犯人 1 本を特定して 4.31 → 3.16s

16 本目が**赤のまま引き継いだ** `--profile full` を始点にした。赤は 2 軸あり
（`guff_only` 4 / wall 4.150s）、**別々の原因**だったので別々に扱った。

#### 1. 引き継ぎの前提が 1 つ間違っていた —— 14 本目もすでに赤

`gate.py` の上限は `baseline × ratio + epsilon` = `2.360 × 1.0 + 0.150` = **2.510s**。
16 本目が「良い側」とした 14 本目（`6a3cd9a`）は **2.98s** で、すでに 0.47s 超えている。
二分探索の述語が単調でなかったので、収束したのは**最後の遷移だけ**だった。
finding のほうは 14 本目が 20 で baseline と一致するので、偽陽性 4 件が
15 本目だけの話であるという結論は正しい。

#### 2. wall の犯人は 13 本目（`b96fd2e`）—— S1008 の再パースが無条件だった

各コミットを建てて 3 回ずつ測った。**run1 は毎回外れる**（ビルド直後でバイナリが
ページキャッシュに無い）ので run2/3 を採る:

| コミット | wall | ill_typed |
|---|---:|---:|
| `c71825b`（8 本目 / 記録に残る最後の実行） | 2.45 / 2.46 | 8 |
| `72af24e` | 2.39 / 2.41 | 8 |
| `fe256bf` | 2.45 / 2.47 | 8 |
| `a9ce168`（12 本目） | 2.53 / 2.55 | 7 |
| **`b96fd2e`**（13 本目） | **2.86 / 2.90** | 7 |

**+0.34s が 1 本に乗っており、ill_typed は動いていない** —— §6（10 セッション分の
誤診）とは違って、これは「正しく増えた仕事」ではない。

中身は 13 本目の「S1008 がコメントを見ていなかった」修正で、`PARSE_COMMENTS`
再パースと `CommentMap` の構築を**パッケージ内の全ファイルで無条件に**やっていた。
`if cond { return true }; return false` の形はほぼどのファイルにも無い。

**同じ形が 15 本目の `fact_deprecated` にもあった。** そして
`inline` / `directive` / `buildtag` は**すでにバイトスキャンのゲートを持っている**
（`inline` のコメントは「almost no files carry `//go:fix inline`」と書いてある）。
直近 2 セッションで入った 2 つだけがその作法を外していた。

#### 3. 偽陽性 4 件 —— 3 件は 15 本目の副作用、1 件は初回 commit から

| linter | 位置 | 原因 |
|---|---|---|
| SA4006 | `tsdb/example_test.go:58` | `example_func_spans` を計算して**一度も参照していなかった**（15 本目で入ったまま未配線） |
| SA9004 | `tsdb/head.go:240` | `group_specs` が上流の `astutil.GroupSpecs`（行の隣接）ではなく「値を持たない spec で切る」だった |
| SA5011 | `tsdb/querier.go:243` | 値文脈の `&&` が分岐に落ちていない（`logicalBinop` 未移植） |
| revive `var-declaration` | `config/config.go:579` | 15 本目が外したゲートに、上流の対応物があった |

**SA9004 は fixture のほうも間違っていた。** `const ( A E = 1; B = 2 )` は
上流が撃たない形（同一行の spec は `End().Line + 1 != Pos().Line` で必ず割れる）で、
guff の旧グルーピングに合わせて書かれていた。4 形（隣接行 / コメント挟み /
空行 / 同一行）を golangci-lint 2.12.2 で実測し、**隣接行だけが報告される**ことを
確認して `bad.go` / `ok.go` を書き直した。§7 の「実際の Go ツールチェインに
読ませていない fixture」が、コンパイルは通る側で出た例である。

**revive `var-declaration` の境界も実測で確定した。** 15 本目は
「上流のゲートは `IsUntypedConst` ただ 1 つ」と書いてゲートを削除したが、
実際に効いているのはその**手前**の `validType(rhsTyp)`
（コメントに `// Type checking failed (often due to missing imports).` とある）で、
revive は自前の `lint.Package` で型検査するので**インポート越しのオペランドは
そこで落ちる**。1 var ブロック 1 形で測った:

```
local1 + local1 / localFunc() / localVar        → 報告する
sub.EscapingKey / sub.Func() / sub.Var / sub.Typed → 黙る
```

**定数性ではなく「RHS が他パッケージに触れているか」が線である。**
消したゲートは広すぎたが、根拠が無かったわけではない。

#### 4. SA5011 の下に SSA の欠落がある（未修正、サイズ付き）

`hints != nil && hints.ShardCount > 0` の deref が無防備に見えるのは、
guff-ssa の `builder::expr::binary_expr` が**両オペランドを現在のブロックに
無条件で吐く**からである。go/ssa は値文脈の `&&` / `||` を `logicalBinop` に回して
`y` 専用のブロックと Phi を作り、honnef の IR はそこに sigma を置く。
`cond.rs` は条件文脈の `&&` だけを分岐に落としている。

当座は構文側で「`x` が nil 比較したポインタの、`y` の中の deref」を抑制した
（`short_circuit_guarded_derefs`）。**ポインタ一致で絞ってあるので
`a != nil && b.F > 0` は報告されたままである。** 恒久修正は `logicalBinop` の移植で、
CFG の形が変わるので単独セッションの頭でやること。

#### 5. 入れた perf 修正と、入れなかったもの

| 修正 | wall |
|---|---|
| （偽陽性 4 件修正後の出発点） | 4.31s |
| S1008 の遅延化 + `fact_deprecated` のバイトゲート | 3.75s |
| testifylint の regex キャッシュ | 3.34s |
| SA4023 の索引化 | 3.21s |

- **testifylint**: `expected_actual_pattern` が `is_expected_value_candidate` の
  2 行上にあり、**アサーション毎に regex をコンパイル**していた。
  regex の*構築*が 0.53s CPU —— 実行中の全 regex マッチより大きい。
- **SA4023**: 候補比較ごとにパッケージ全体を再走査していた。
  **全走査 467M ノードのうち 267M**（57%）を単独で占めていた。索引 1 回に。
- **gocritic `type_implements`**: 呼び出し毎に `TypeArena` 全体を clone。
  メモ化したが **wall は 1% しか動かなかった**（CPU 17.85 → 17.71s）。

**入れなかったもの: `InspectResult` の kind 索引。** preorder の
「202M 走査して 97% を mask で捨てている / analyze CPU の 22.9%」という数字から
kind ごとの索引を作ったが、**wall は 3.21 → 3.19s でノイズ内**だった。
2.89s という数字自体が `GUFF_DEBUG_CACHE=2` の計測オーバーヘッドを含んでおり、
素の線形走査（16 バイト × 連続配列）はもともと安かった。
`PERF_TASKS.md` の「狙った phase の秒数が実際に下がったか。下がっていないなら
入れる意味がない＝再考」に従って戻した。**計測器が測っている対象を太らせる例。**

#### 6. baseline は再ロックしていない

finding は **20/20/20 P=R=1.0** に戻り、ゲートの finding 軸は緑。
wall だけ 3.16s（限界 2.510s）で赤のまま残す。

残っている差は 2 種類の合計で、**まだ分けられていない**:

1. **正しく増えた仕事** —— baseline を刻んだ時点（5 本目）に対し、
   ill_typed は 8 → 3（5 パッケージ分の解析が増えた）、`fact_deprecated` は
   自パッケージについて**何も出力していなかった**、`preorder` の 4 箇所が
   走査を途中で打ち切っていた、SA1019 の deprecation ガードが不活性だった。
2. **まだ無駄な分** —— CPU 上位は gocritic 1.06s / QF1008 1.03s / buildir 0.88s /
   SA5009 0.79s / godot 0.67s / SA1019 0.66s / unconvert 0.65s / revive 0.64s。

**ただし CPU を削っても wall はあまり動かない**ことが 2 回続けて確認できた
（gocritic のメモ化、kind 索引）。ワーカーは wall 3.2s に対して 1.3s しか
回っていない。次に効くのは CPU の総量ではなく**クリティカルパス**
（`typecheck_roots` 1.04s と analyze の最も遅いパッケージ）である。
per-package の時間を出す計測がまだ無いので、そこから。

**次にやること**

0. **wall は赤のまま。** 上記 6 の 2 分割を先にやること —— 「正しく増えた分」が
   何秒かを出さないと、baseline を上げるべきか下げる余地があるかが決まらない。
   per-package の analyze 時間を `GUFF_DEBUG_CACHE=2` に足すのが最初の一手。
1. **`logicalBinop` の移植**（上記 4）。SA5011 の構文側の当て木が外せる。
2. `MakeInterface` にオペランドを持たせる（15 本目からの持ち越し、優先度そのまま）。

### 2026-08-14（18 本目）— 17 本目の「次にやること」3 本を全部畳み、SSA の欠落を 1 つ**新しく**名指しした

17 本目が残した 0（per-package 計測）・1（`logicalBinop`）・2（`MakeInterface`）を
順に片付けた。副産物として、**この 3 本の前提だった診断が 1 つ間違っていた**ことと、
その下にもう 1 段の欠落があることが分かったので、そこも実測つきで置いておく。

#### 1. `MakeInterface` は「オペランドが無い」のではなく、**一度も発行されていなかった**

15 本目から「`pub struct MakeInterface {}` がボクシングされる値を持たないので
referrer が張れない」と記録してきた。オペランドを足して測ったら、
**`sa4006/ok.go` の偽陽性が消えなかった**。IR を出すと理由がすぐ出た:

```
func interfaceBoxing(n int):        ← 修正前
	t0 = println(1)
	t1 = println(n)
	return
```

変換命令が 1 つも無い。`var i interface{} = 1` の代入は
`lvalue::Address::store` → `Builder::emit_store` → `emit::emit_store` と降りるが、
**`emit_store` が値を素通しで格納していた**。go/ssa の `emitStore` は
`Val: emitConv(f, val, MustDeref(addr.Type()))` である ——
つまりインターフェースへのボクシングが起きる場所そのものが、guff には無かった。
`MakeInterface` が空構造体だったのは症状であって原因ではない。

直したのは 3 か所:

| 場所 | 内容 |
|---|---|
| `instr.rs` | `MakeInterface { x, typ }` / `ChangeInterface { x, typ }`。**両方とも空構造体で、両方とも一度も emit されていなかった** |
| `emit.rs::emit_conv` | 上流の `isNonTypeParamInterface(typ)` の枝を移植。iface → iface は `ChangeInterface`、untyped nil は interface 型の nil 定数、その他の untyped は既定型へ 1 段変換してから `MakeInterface` |
| `emit.rs::emit_store` | 格納先のポインタ先型へ `emit_conv`（= 上流の `emitStore`） |

```
func interfaceBoxing(n int):        ← 修正後
	t0 = make interface{} <- int (1)
	t1 = println(t0)
	t2 = make interface{} <- int (n)
	t3 = println(t2)
	return
```

**落ちた 2 件は、どちらも「上流の作法を guff が別の形で肩代わりしていた」側だった。**

- **SA1014** が 3 件 → 2 件に落ちた。落ちたのは `var i1 any = v; json.Unmarshal(data, i1)`
  の形 —— 代入で箱に入るようになった結果、`i1` の値が `MakeInterface`（型は `any`）に
  なり、`Pointer()` 述語（`*types.Pointer | *types.Interface`）を通ってしまう。
  上流の `callcheck.checkCalls` は `Call.Args` を組む前に
  `if iarg, ok := arg.(*ir.MakeInterface); ok { arg = iarg.X }` と**箱を剥がしている**。
  golangci-lint 2.12.2 に 5 形を読ませて確認した —— map・map を入れた `any` 変数・struct、
  `Unmarshal` と `Decode` の**全部が報告される**。`interface` を許す述語で
  `any` 変数まで報告されるのは、剥がしている場合だけである。
  （`json.Unmarshal(data, v)` に map を直接渡す形は前後で変わらない。
  guff は呼び出し引数を仮引数型に変換しないので、そこには箱が無い —— 下記 5。）
- **contextcheck** の `contextcheck_nocapture` が黙った。
  `func fromClient() listFunc { return func(ns string) error {…} }` は
  戻り値型が名前付きなので、**return で `changetype` が挟まる**（go/ssa も同じ）。
  `instr_callees` が `Return.results` の中に素の `Value::Function` を探していたので、
  1 命令ぶん奥に入っただけで追跡が切れた。`ChangeType` を剥がすようにした。

`crates/guff-ssa/tests/emit_test.rs` も落ちた。テストが
**空のアリーナに transmute した `Value::Global`** を渡していて、
`emit_store` が格納先の型を読むようになった瞬間に添字が範囲外になる。
go/ssa の `emitStore` も `addr.Type()` を読むので、これは直す側。実 universe に書き換えた。

**結果**: golden の staticcheck-sa ratchet を **extra 7 → 6**。
`compat/golden/cases/staticcheck-sa/ratchet.json` から SA4006 の行を落とした。

#### 2. `logicalBinop` を移植し、SA5011 の当て木を外した

17 本目が「恒久修正は `logicalBinop` の移植、CFG の形が変わるので単独セッションの頭で」と
書いていたもの。値文脈の `x && y` / `x || y` に `binop.rhs` ブロックと
`binop.done` の Phi を作る（短絡側の辺は `false` / `true` 定数、`y` 側の辺は `y` の値）。
上流と同じく、`rhs` に前任者がいなければ短絡定数を返し、`done` に前任者がいなければ
`y` をそのまま返す。

これで 17 本目の `short_circuit_guarded_derefs`（構文から guard を復元していた当て木）を
**丸ごと削除**できた。prometheus `./tsdb/...` は SA5011 **0 件**。
4 形を golangci-lint 2.12.2 と並べて実測し、両者一致を確認した:

| 形 | 上流 | guff |
|---|:--:|:--:|
| `n := a.N` の後に `if a == nil` | 報告 | 報告 |
| `if ok && a.N > 0`（左辺が `a` を見ていない） | 黙る | 黙る |
| `if a != nil && a.N > 0` | 黙る | 黙る |
| `ok := a != nil && a.N > 0`（値文脈） | 黙る | 黙る |

#### 3. インターフェースのメソッドにレシーバを繋いだ（6 本目からの持ち越し）

§7 が「設計判断ではなく欠落」と書いていたもの。`interface_set_method_receivers` /
`interface_repoint_method_receivers` を足して 3 か所で呼ぶ。

**上流自身が 2 通りに綴る**、というのがこの件の要だった:

| 由来 | 上流の綴り | なぜ |
|---|---|---|
| ソース検査した `type T interface{…}` | `(pkg.T).M` | `Checker.interfaceType` が `def` を受け取り、名前付き型をレシーバにする |
| export data から読んだもの | `(interface).M` | `ureader` はレシーバ無しで作り、`types.NewInterfaceType` が**インターフェース自身**を入れる。`writeFuncName` は `*types.Interface` を見ると型を書かず `interface` と綴る |

guff は `def` を `typ` に通していないので、順序を逆にした ——
インターフェースを自分自身をレシーバとして建て、`type T interface{…}` の宣言側で
`named` に**付け替える**。付け替えを `from == iface` で絞ってあるので、
入れ子のリテラルも `type T U`（`U` が名前付きインターフェース）も上流どおり動かない。

**errcheck の別名は片方だけ消えた。** `build_exclude_set` が
`(pkg.T).M` を見るたびに足していた 2 つのうち、`pkg.M` は不要になったので削除した。
`(interface).M` は**残す** —— 回避策ではなく、上流 errcheck が
`namesForExcludeCheck` / `walkThroughEmbeddedInterfaces` で
**選択の受け手型から**名前を組み立てていることの代役だからである。
`(io.Writer).Write` を除外する config を両ツールに読ませて、削除の前後で一致を確認した。

**結果**: golden の errcheck-verbose ratchet **1/1 を削除**（0/0）。

#### 4. per-package の analyze 時間 —— wall を決めているのは 1 パッケージだった

17 本目の「次にやること 0」。`GUFF_DEBUG_CACHE=2` に per-package の表を足した
（summed CPU / action 数 / **最初の action が始まってから最後が終わるまでの span**）。
prometheus `./...`:

```
guff: per-package analyze time (top 20 of 114 pkgs; 10.91s total CPU, 1.49s from first action to last):
       2.90s CPU     206 actions  [  0.00s..  1.49s]  .../prometheus/tsdb
       0.60s CPU     205 actions  [  0.00s..  1.47s]  .../prometheus/web/api/v1
       0.55s CPU     202 actions  [  0.00s..  1.48s]  .../prometheus/storage/remote
       …
  tail: .../prometheus/tsdb ends at 1.49s (2.90s CPU over 1.49s span)
guff: phase typecheck_roots 1.06s (114 pkgs, 114 analyze roots)
guff: phase analyze (run_on_packages) 1.54s
```

**analyze の wall は `tsdb` 1 パッケージで決まっている。** CPU 合計 10.91s に対し
span は 1.49s（≒7.3 並列）だが、`tsdb` だけで 2.90s CPU を 1.49s の span に
押し込んでおり（≒1.95 並列）、しかもその span が phase 全体を覆っている。
2 番手の `web/api/v1` は 0.60s CPU、つまり **`tsdb` を除くと analyze は 1s を切る**。

これで 17 本目が 2 回続けて観測した「CPU を削っても wall が動かない」
（gocritic のメモ化 CPU 17.85 → 17.71s で wall 1%、kind 索引で wall 3.21 → 3.19s）が
数字になった。**削るべきは CPU 総量ではなく `tsdb` 上の 1 パッケージ分の CPU**、
あるいはパッケージ内 action DAG の幅である。

**上の秒数を 17 本目の 3.16s と直接引き算しないこと。** この測定を採った時点の
作業ツリーには**別セッションが進行中の `TypeArena` の overlay を `Arc` 化する変更**
（`arena.rs`、PERF_TASKS_V3 V1-1）が同時に入っていた。分けて測っていないので、
17 本目からの差のどれだけがそちらのものかは**この記録では答えられない**。
（その後そちらは `.claude/worktrees/perf-v3` に移り、main のツリーからは消えている。）
per-package の表そのもの（どのパッケージが tail か、CPU と span の比）は
どちらの変更にも依存しないので、上の読みは有効である。

#### 5. その下にもう 1 段: **`emitCallArgs` が無い**

`MakeInterface` を直せば controller-runtime の unparam 2 件と
その `//nolint:unparam` の nolintlint 1 件も消える、と allowlist に書いてあった。
消えなかった。上流 unparam の表は
`addImplementing(findNamed(instr.X.Type()), iface)` を**全 `MakeInterface` について**回すが、
実際の変換地点が `WithValidator(&podValidator{})` ——
**呼び出しの引数**だからである。そして:

```go
func viaArg(t T)    { take(t) }                 // take(i I)
func viaAssign(t T) { var i I = t; take(i) }
```

```
func viaArg(t T):        guff: t0 = take(t)                    go/ssa: t0 = make I <- T (t) / t1 = take(t0)
func viaAssign(t T):     guff: t0 = make I <- T (t) / t1 = take(t0)
```

`builder/call.rs` の `c.args.push(self.expr(arg))` が
**引数を仮引数型へ変換していない**。go/ssa の `emitCallArgs` は
`emitConv(fn, args[i], sig.Params().At(i).Type())` を通常引数ぶん回し、
可変長引数はスライスに詰め直す。

unparam 側に SSA から `typesImplementing` を組む実装を書いて回してみたが、
上のとおり表が空になり**観測可能な差が 0**、buildir 依存だけが増えるので**入れずに戻した**。
先に `emitCallArgs` を入れること。allowlist の「SSA を 1 つ直すと 4 件」は正しくなく、
**2 つ**である: `MakeInterface` のオペランド（済）と `emitCallArgs`（未）。

#### 6. ゲートの状態

`cargo test` **3116 green**。golden **81 ケース一致**（ratchet: errcheck-verbose を削除、
staticcheck-sa を extra 7 → 6 に低下）。reject **12 ケース**。isolate tier **116 ターゲット**緑。
OSS pr + nightly tier **10 ターゲット**緑（recall は全ターゲットで 100%、
`unexpected_guff` / `unexpected_golangci` とも 0、health は 3 ターゲットとも baseline どおり）。

**regress full の wall は 19 本目の末尾で測れた**（18 本目の時点では perf ガードが
2 回とも contended で拒否していた）。結果は下記 7。

**次にやること**

1. **`emitCallArgs` の移植**（上記 5）。呼び出し引数すべてに変換が入るので blast radius は
   今回の `emit_store` より大きい —— 単独セッションの頭でやること。
   controller-runtime の unparam 2 件 + nolintlint 1 件がこれで閉じる（はず。閉じたら実測すること）。
2. **wall**: 上記 4 のとおり `tsdb` 1 パッケージ。per-analyzer の表を
   **パッケージで絞れる**ようにするのが次の一手（今の表は全パッケージ合算なので、
   `tsdb` の 2.90s の内訳が読めない）。**まず `regress/run.sh --profile full` を
   空いた機械で 1 回通すこと** —— 18 本目は perf ガードに 2 回とも止められていて、
   17 本目の 3.16s から動いたかどうかすら分かっていない。
3. `replaceRecvType`（`subst.rs`）—— インスタンス化したジェネリックインターフェースの
   メソッドが、レシーバとして**元の**インターフェースを指したままになっている。
   上流は Func と Signature を複製する（メソッドはインスタンス間で共有されるため）。
   読むのは名前だけ（`identical` はレシーバを見ない）なので優先度は低い。

### 2026-08-14（19 本目）— `emitCallArgs` を入れて、18 本目が名指しした残り 1 段を塞いだ

18 本目が「次にやること 1」として実測つきで置いていったもの。呼び出し引数が
仮引数型へ変換されないので `take(t)` に `MakeInterface` が出ない、という欠落。

#### 1. 移植したもの／**意図的に移植しなかったもの**

`builder/call.rs` の `set_call` と `emit_call` が共通の `emit_call_args` を通り、
評価した実引数を仮引数型へ `emit_conv` する。上流 `emitCallArgs` と同じく
`offset`（具象レシーバが既に積まれていれば 1）を起点にする。

**可変長引数のスライス構築は移植していない。** 上流は末尾を配列 + `Slice` に
詰め直すが、guff は個別に渡して `CallCommon::ellipsis` で spread を記録する既存の
規約があり、こちらの analyzer は全部それを読んでいる。そこで末尾は
**可変長仮引数の要素型**へ変換する —— 上流が作るスライスの中で各引数が持つ型である。

```
func viaVariadic(t T, n int):        takeAny(a ...interface{})
	t0 = make interface{} <- T (t)
	t1 = make interface{} <- int (n)
	t2 = takeAny(t0, t1)
func viaSpread(xs []interface{}):    takeAny(xs...)  ← 素通し
	t0 = takeAny(xs)
```

**多値の連鎖（`f(g())`）は変換しない。** 上流は `emitExtract` でタプルを平らにするが
guff は 1 引数のまま渡すので、実引数と仮引数の個数が合わない。数が合わないときは
**何も変換しない**（合わないまま変換すると別のオペランドを強制することになる）。
`DEFERRED` として `convert_call_args` に書いてある。

builtin（`Value::Builtin`）も飛ばす。上流は builtin を専用の lowering に回すので、
合成したシグネチャで変換するのは間違いになる。

#### 2. golden が `isValuePreserving` の欠落を 1 つ出した

入れた直後、`sa1017/bad` が **missing** に落ちた（SA1017: signal.Notify に渡す
チャネルはバッファすべき）。`signal.Notify(c chan<- os.Signal, …)` に
`chan os.Signal` を渡すと変換が入るが、guff の `emit_conv` は
「underlying が identical か」しか見ていないので **`Convert` を出していた**。
上流の `isValuePreserving` は

```go
switch ut_dst.(type) {
case *types.Chan:    _, ok := ut_src.(*types.Chan); return ok
case *types.Pointer: _, ok := ut_src.(*types.Pointer); return ok
}
```

と、**チャネル間・ポインタ間は値を保存する**と答える ＝ `ChangeType`。
`Convert` にしてしまうと、`ChangeType` を辿って元の値に戻る検査
（`flatten_ssa_value`）から演算子が見えなくなる。移植して golden は 81 一致に戻った。

**これは `emitCallArgs` が無ければ一生踏まなかった欠陥である。** 変換が起きる場所が
無ければ、変換の分類が間違っていても観測できない。

#### 3. 閉じたもの

`compat/allowlists/controller-runtime.txt` の **unparam 2 件**と、その
`//nolint:unparam` が「未使用」に見えていた **nolintlint 1 件**。
18 本目が書いて戻した SSA 版 `typesImplementing` を入れ直したら、今度は表が埋まった:

```
typesImplementing[.../examples/builtins] = [… "podValidator.ValidateCreate",
    "podValidator.ValidateDelete", "podValidator.ValidateUpdate", …]
typesImplementing[.../examples/tokenreview] = ["Webhook.ServeHTTP", "authenticator.Handle"]
```

AST 側の `collect_interface_methods`（名前 + シグネチャ一致）は**残してある**。
IR 側は「実際に変換された型」しか見ないので、このパッケージで宣言されているが
一度も変換されないインターフェースを取りこぼす。上流は SSA だけで判断するが、
guff の IR はパッケージ単位なので両方要る。

#### 4. 落とし穴（記録）—— `guff run` は issues キャッシュを持っている

「直したのに出力が変わらない」を 3 回繰り返した。原因は analyzer ではなく
**永続 issues キャッシュ**で、同じツリーの 2 回目以降は analyzer が走らない。
デバッグ用の `eprintln!` すら出ないので「コードが呼ばれていない」ように見える。
**手で確認するときは `--no-cache` を付けること。** ゲート（`compat/run.sh` /
`golden/run.sh`）は毎回新しい結果ディレクトリを使うのでこの影響を受けない。

#### 5. tail パッケージの内訳を出した —— 犯人は QF1008 と unconvert

18 本目の「次にやること 2」。`(package, analyzer)` の集計を足して、
**span が最後に終わるパッケージ**について上位 15 analyzer を出すようにした。
prometheus `./...`（`--no-cache`）:

```
  tail: .../prometheus/tsdb ends at 3.25s (8.75s CPU over 3.25s span)
  tail breakdown (top 15 analyzers in that package):
                            QF1008    1.843s   21.1%
                         unconvert    1.234s   14.1%
                            SA5009    0.771s    8.8%
                          gocritic    0.727s    8.3%
                           buildir    0.418s    4.8%
                             godot    0.370s    4.2%
```

**QF1008 と unconvert だけで、analyze の wall を決めているパッケージの 35%。**
全パッケージ合算の表では QF1008 は上位 20 に入っておらず（gocritic / buildir /
SA5009 が上に来る）、**この 2 本は `tsdb` に集中している**ことが初めて見えた。
17 本目が 2 回とも「CPU を削っても wall が動かない」で終わったのは、
合算表を見て**合算表の上位**を削っていたからである。

**絶対値は信用しないこと。** この測定時の load average は 3.9 で、
`regress/run.sh` の perf ガードなら測定を拒む水準である。比率
（どの analyzer が tail パッケージの何 % か）は使えるが、秒数は使えない。

#### 6. golden の ratchet を extra 6 → 2 に落とした —— そのうち 1 本は「検査が丸ごと反転」していた

残っていた staticcheck-sa の差分を 1 件ずつ実測して潰した。

| 差分 | 実態 |
|---|---|
| SA1023 の位置 | guff は**関数名**に 1 件、上流は**書き込んでいる命令**に 1 件ずつ。2 行書き換える `Write` で上流は 2 件、`_ = append(b, 1)` は `append` の位置（`_` ではない） |
| SA4020 の文言 | 空インターフェースの節を "earlier case" と綴っていた。上流は**書かれたとおりの型名**（`case any:` は `any`、`case interface{}:` は `interface{}`） |
| SA9004 の列 | グループの**型**の位置に報告していた。上流は `group[0].Pos()` ＝ 最初の**名前** |
| **SA4015** | **反転していた。** IR の腕が `ir.Convert` ではなく `ChangeType` を見ていて**recall が 0**、その代役の AST 経路が `math.Ceil(1)` に当たっていた —— 上流は定数を報告しない（定数は既に `float64` で、変換が存在しない）。IR の腕だけに書き直し、7 形が上流と完全一致 |

**SA4015 の教訓は「extra 1 件」に見えていたものが「recall 0 + 誤検出 1」だった**こと。
golden の ratchet は**両側を 1 行ずつ**しか見せないので、
`bad.go` の fixture が上流の撃たない形になっていると、
**recall の穴が extra 1 件に化けて見える**。

**fixture を書き換えたら golden を再生成すること。** SA4015 の fixture を直したら、
17 本目が書き換えた **SA9004 の fixture 由来の行が同時に出てきた** ——
つまりあのとき golden を再生成しておらず、**ratchet の余裕がそれを吸収して隠していた**。
再生成の差分は追加 5 行のみで、削除は無い（レビュー済み）。

残った 2 件（SA4031 / SA5005）は**どちらも SA4015 と同じ反転**で、直すには移植が要る:

- **SA4031**: 上流は 5 形を報告する（`make` / `new` / スライスリテラル / 関数値 /
  アドレス取得、いずれも変数名を挙げる related information つき）。guff は
  **その 5 形を全部落とし**、上流が黙る `make(chan int) == nil` のインライン形だけを報告する。
- **SA5005**: 上流は**何も報告しない** —— 教科書どおりの
  `runtime.SetFinalizer(x, func(_ *int){ _ = x })` すら。guff は報告する。
  上流がどの条件でだけ撃つのかを honnef のソースで確かめるところから。

#### 7. wall を 3 セッションぶりに測った —— SSA の忠実化は wall を動かしていない

`regress/run.sh --profile full`（load 1.94 の静かな機械で 1 回）:

| 指標 | baseline | 17 本目 | 19 本目 |
|---|---:|---:|---:|
| wall_seconds | 2.360 | 3.160 | **3.210** |
| peak_rss | 2.90 GiB | — | 3.14 GiB（上限 ×1.20 = 3.48 GiB 内） |
| finding | 20/20/20 | 20/20/20 | **20/20/20 P=R=1.0** |

**18・19 本目で入れた SSA の 3 変更はどれも命令を増やす側**
（`emit_store` の変換、`logicalBinop` のブロックと Phi、`emitCallArgs` の変換）
**なのに、wall は +0.05s しか動いていない** —— ゲート自身が測定ノイズとして
許している epsilon 0.15s の中である。つまり **baseline との 0.85s 差は
このセッション群の作業由来ではない**。上記 5 の tail 内訳（QF1008 21% /
unconvert 14%）が引き続き唯一の具体的な打ち手である。

wall 軸は**赤のまま**。finding 軸は緑。

#### 8. SA4031 を移植し、SA5005 は「条件は合っているが IR が違う」と分かった

上記 6 が「どちらも SA4015 と同じ反転」と書いた 2 件。

**SA4031 —— 完全一致（5 形）。** 上流は `*ast.IfStmt` **だけ**を歩き、
`nil` は**右辺**でなければならず、`&x == nil` は SA4022 に譲る。そのうえで
IR を `MakeChan` / `MakeMap` / `MakeSlice` / `Alloc` / `Function` /
`MakeClosure` / `Slice` / `FieldAddr` / `Phi` まで遡って never-nil を証明する。
guff は**逆**で、`if` に限らず撃ち、その遡りを持っていなかった。

**これを直すには guff-ssa 側も 1 つ足りなかった: `new(T)` が Alloc に落ちていない。**
`t0 = new(nil)` という builtin 呼び出しのままで、
「このポインタはどこから来たか」を訊く検査からは見えない。
go/ssa の `builder.builtin` は `emitNew(fn, mustDeref(typ), pos, "new")` である。

測るときの罠が 1 つ: **golangci-lint の `issues.max-same-issues` は既定 3** なので、
同文言の 4 件目以降が消える。関連情報だけが残って所見が消えるので
「上流は撃たない」と読み違える。`max-same-issues: 0` を置いて測ること。

**SA5005 —— 条件は移植した。差分は IR の形。** 上流の条件は 3 つとも厳密で
（オブジェクト引数が **Alloc の Load**、finalizer 引数が **MakeClosure**、
その binding に**同じ Alloc** がいる）、guff もそのとおりに実装した。
それでも guff は撃ち、上流は撃たない —— **guff の IR が条件を満たし、
honnef の IR が満たさない**からである。上流は
`x := &Foo{}; runtime.SetFinalizer(x, func(y *Foo){ … x … })` という
**自分のドキュメントの例でも報告しない**。
これは検査の欠陥ではなく IR の差なので、ratchet に理由つきで残した。
（上流の文言末尾 `(at %s)` は、比較できる上流の所見が存在しないので付けていない。）

**ratchet は extra 6 → 1**（19 本目の開始時からの通算）。

**次にやること**

1. **QF1008 と unconvert を `tsdb` の上で読む**（上記 5・7）。合算表ではなく
   tail の内訳が打ち手を決める、というのが 18・19 本目で分かったこと。
   wall の 0.85s はここ以外から出ていない。
2. ~~**wall の実測**~~ 済み（下記 7）。**3.210s** —— 17 本目の 3.160s から
   +0.05s、ゲート自身のノイズ許容（epsilon 0.15s）の中。
3. 残っている missing 3（SA6001 の recall、SA5011 の σ、SA6000 まわりの SA4006）。
4. `replaceRecvType`（`subst.rs`）。優先度低のまま。

---

### 2026-08-14（20 本目）— 犯人は当たっていたが、同じ日に別のワークストリームがもっと広く直していた。残ったのは「tail は働いていない、待っている」

19 本目の「次にやること」1・2。**このセッションの成果物は計測器 1 つと、それが出した所見**である。
最初に書いた修正は**測って捨てた** —— 以下はその経緯も含めて書く。

**worktree で作業する場合の準備**（並行セッションと混ざらないために推奨）:
`.cargo/config.toml` も `CARGO_TARGET_DIR` も無いので `target/` は worktree ごとに
自動的に別になる（フルビルドが 1 回要る）。一方で**ゲートの入力は gitignore されていて
worktree には来ない**ので、本体から symlink を張る必要がある ——
`prometheus`（`regress/`）、`corpus/cache`（OSS tier）、`compat/corpus`、`compat/.tools`。
`.gitignore` の該当行は末尾が `/` なので**symlink は無視されず untracked に見える**。
コミットしないよう `git add <path>` を明示すること（`git add -A` を使わない）。

#### 1. QF1008 と unconvert の正体は当たっていた —— `TypeArena::clone` が 8 割

`cargo build --profile profiling` + `sample(1)`（§9.4）で、部分木のサンプルを
**自己時間でシンボル別に**集計した:

| 部分木 | サンプル | うち `TypeArena` の clone + drop |
|---|---:|---:|
| `qf1008::run` | 1089 | **~79%** |
| `unconvert::run` | 754 | **~86%** |

内訳は `arena.rs:133`（`Layered<TypeData>` の複製）、`arena.rs:137`（`intern_overlay` の複製）、
`drop_in_place<Vec<TypeData>>`、`drop_in_place<RawTable<(InternKey, TypeId)>>`。
**検査そのものはほとんど動いていない。**

原因は 1 行。`lookup_field_or_method` と `identical` はメソッド集合と型集合を遅延キャッシュ
するので `&mut TypeArena` を要求するが、パッケージのアリーナは共有で渡ってくる。そこで
**呼び出しごとに `artifacts.types.clone()`** していた。clone は base を `Arc` で共有する一方
**overlay（そのパッケージ自身が確保した型）は実体コピー**するので、
**1 回のコストがパッケージの大きさに比例する** —— 合算表で上位に来ないのに tsdb で
21% / 14% を占めていた理由がこれである。**合算は、まさにこの形の欠陥を平均で薄めて隠す。**

パッケージごとに 1 回だけ clone して `&mut TypeArena` を引き回す形に直し、
tsdb の CPU 4.48s → 3.04s、analyze phase 1.78s → 1.67s、findings 完全一致まで確認した。
**この 4 つの数字はすべて統合前（b5dbcb8）の上のもの**で、下の 3 に出てくる
統合後の数字（analyze 0.93s）とは土台が違う。並べて読まないこと。

#### 2. そこで push しようとしたら、main に 10 コミット載っていた

**同じ日に別のワークストリームが同じ根本原因に到達していて、しかも直し方が広かった。**
`Layered::overlay` と `intern_overlay` を `Arc` にして
（`docs/PERF_TASKS_V3.md` の **V1-1**）、**`TypeArena::clone` を参照カウント 2 回にした** ——
2 か所ではなく**約 30 か所の呼び出し側が、コード無変更のまま**タダになる。

rebase したうえで、`scripts/perf-ab.sh`（同じセッションが入れた交互 A/B ハーネス）で
**A = 統合後、B = 統合後 + 自分の 2 analyzer 修正**を測った:

```
perf-ab --mode cpu --rounds 6
  A cpu: median 16.245   B cpu: median 16.330
  delta: +0.085 (+0.5%)   min-to-min +0.000
```

`GUFF_DEBUG_CACHE=2` を A/B/A/B と 3 往復させても、analyze phase は
A 1.02/1.04/1.03 対 B 1.01/1.03/1.06 で**区別がつかない**。tsdb の tail 上位 15 から
**QF1008 も unconvert も両側で消えている**。

→ **自分の修正は統合後の状態でゼロだった**ので、**捨てた**。V1-1 が完全に上位互換である
（interning が呼び出しをまたいで残るぶんだけ理屈では得だが、測って出ない差は入れない）。
**残したのは下の計測器だけ。**

**教訓として残す価値があるのはここ**: 「プロファイルが指した犯人が正しい」ことと
「自分の直し方が要る」ことは別である。**rebase 後に測り直す**という手順が無ければ、
効果ゼロの引数引き回しを 2 ファイルに永久に残していた。

#### 3. per-analyzer 表に span を足した —— そして「tail は働いていない、待っている」と分かった

19 本目の「次にやること」2。`ANALYZER_BY_PACKAGE` の値を `u128` から
`PkgTiming`（CPU ＋ first_start/last_end）に変え、tail の内訳に `[start..end]` を出し、
**「そのパッケージで最後に終わる 5 本」を `waited` つきで**並べるようにした
（`waited` ＝ パッケージの最初のアクションからその analyzer が始まるまでの空き。
`format_checks waited` と同じ語彙）。`GUFF_DEBUG_CACHE` 無指定時のコストはゼロ。

統合後の tsdb（1.85s CPU / 0.90s span、analyze phase 0.93s）:

```
  tail breakdown (top 15 of 206 analyzers in that package, by CPU; [start..end] …):
                          gocritic    0.241s   13.0%  [  0.38s..  0.62s]
                           buildir    0.206s   11.2%  [  0.13s..  0.34s]
                             godot    0.184s    9.9%  [  0.45s..  0.64s]
  tail critical path (last 5 analyzers to finish in that package):
                            SA9005 [  0.86s..  0.87s]     0.006s CPU (  0.3%)  waited   0.86s
                            SA4010 [  0.82s..  0.87s]     0.044s CPU (  2.4%)  waited   0.82s
                            SA4006 [  0.81s..  0.87s]     0.056s CPU (  3.0%)  waited   0.81s
                       ineffassign [  0.83s..  0.88s]     0.045s CPU (  2.5%)  waited   0.83s
                            SA4017 [  0.89s..  0.90s]     0.010s CPU (  0.6%)  waited   0.89s
```

**span を決めている 5 本は、合計 0.16s しか働いていない。0.81〜0.89s は待ちである。**
この 5 本を全部 0 にしても span は 0.90 → 0.89s にしかならない。

**待ちの理由を `requires` で確かめた。** SA4006 / SA4010 / SA9005 は
`buildir`（と `inspect`）**しか**要求しておらず、その `buildir` は
**0.34s で終わっている**。にもかかわらず 3 本とも 0.81s 以降に始まる ——
**この 3 本は依存ではなく順序で待っている**（tail パッケージの残りアクションが
スケジュールの最後尾に回されている）。SA4017 だけは `purity` も要求するので、
最後の 0.03s は本物の依存かもしれない —— **そこは未確認**。

つまり analyze の残りの tail は「analyzer の CPU を削る」とは**別軸**にある。
効くのは**順序**で、tail パッケージのアクションを優先的に流せば span は
buildir 終了（0.34s）＋ 実働（0.16s）の側に寄る余地がある。

**同時に、CPU 表だけを見て着手してはいけないことも表に出た。** gocritic は tsdb の
CPU 1 位（13.0%）だが **0.62s で終わっている**。丸ごと 0 にしても wall は動かない。
17 本目が 2 回とも「CPU を削ったのに wall が 1% しか動かない」で終わったのは、
この区別を**測る手段が無かった**からである。今はある。

#### 4. 残っている missing 3 を全部 golangci-lint 2.12.2 で実測した

19 本目の「次にやること」3 の前半。**3 件とも原因まで特定した**（移植は未着手）。
測定は golden の case config そのまま（`max-same-issues: 0` /
`max-issues-per-linter: 0` 済み）＋ `compat/golden/.work/staticcheck-sa/`。

**SA5011 `sa5011/ok/ok.go:89:17` —— `func_has_interface_param` が条件を取り違えている。**
上流は `okSequentialConcreteFatal(t fataler, statusResp *Status)` を**報告する**
（related information は `86:5`）。guff は報告しない。guff の
`block_has_soft_abort_call` は「**囲む関数がインターフェース型の引数を持つか**」で
soft-abort を判定していて、`fataler` が具象 struct なのでこの経路に入らない。
上流の基準はそこではなく**呼ばれた `Fatal` が実際に noreturn か**である:
`(*testing.T).Fatal` は `runtime.Goexit` に落ちるので `ctrlflow.NoReturn` が noreturn と答えて黙り、
ユーザ定義の `fataler.Fatal`（空の本体）は**戻る**ので撃つ。インターフェース経由は
callee 不明 ＝ noreturn でない ＝ 撃つ。実測でも **`bad.go:23`（TB）と `ok.go:89`（具象）の
両方を上流は撃っている**。guff は前者だけ。
**fixture のコメント（「check stays clean」）が上流と逆で、そこが誤りの出発点。**
直すには引数の型ではなく **callee の noreturn 性**が要る（`panic` / `os.Exit` /
`runtime.Goexit` / それらに落ちる関数、の推移閉包）。guff には今 `lostcancel` の
名前ベースの代用しか無いので、そこから作ることになる。**精度に効くので OSS ゲートで守ること。**

**SA4006 `sa6000/ok/ok.go:13:5` —— `if` の 2 つの後続が両方そのまま関数の出口に落ちると、
上流の IR は分岐ごと畳んで条件値の参照者を消す。** 3 関数のプローブで確定させた:

```go
func tail(lines []string) {                    // 報告される
	if match, _ := regexp.MatchString(`b`, lines[1]); !match { return }
}
func notTail(lines []string) {                 // 報告されない
	if match, _ := regexp.MatchString(`b`, lines[1]); !match { return }
	println("after")                       // ← 1 文足すだけで消える
}
func middle(lines []string) {                  // 2 つ目だけ報告される
	if match, _ := regexp.MatchString(`a`, lines[0]); !match { return }
	if match, _ := regexp.MatchString(`b`, lines[1]); !match { return }
}
```

then 側も else 側も**空のまま関数の出口へ行く**とき両者は同じブロックに融合し、
`If` は後続が同一になって `Jump` に置き換わる。条件値の参照者が 0 になり
SA4006 が「never used」と言う。1 文でも後ろにあれば融合しないので消える。
**stub は関係ない**（golden は本物の `regexp` を使う。`sa6000/stub/` は
`tests/support.rs::collect_stubs` 経由で Rust の単体テストだけが読む）。
移植先は SA4006 ではなく **guff-ssa のブロック整理**。

**SA6001 `sa4006/bad/bad.go:29:6` —— 死んだ `s = string(b)` に上流が撃つ。** 実測の切り分け:

| 形 | 上流 |
|---|---|
| `k := string(bs); return m[k]` | **撃つ**（教科書どおり） |
| `s := "a"; _ = s; s = string(bs)`（golden の形。値は読まれない） | **撃つ** |
| `s := string(bs); _ = s` | 撃たない |
| `return string(bs)` / `sink(string(bs))` | 撃たない |
| `k := string(bs); sink(k); return m[k]` | 撃たない |

`[]byte → string` 変換の**参照者の集合**で決まっている。死んだ再代入が撃たれて
死んだ宣言が撃たれない差はまだ説明できていない。`honnef.co/go/tools` の
`CheckMapBytesKey` の実物を読むところから（module cache に無いので取得が要る）。

**次にやること**

1. **tail パッケージのアクションを優先的にスケジュールする**（上記 3）。
   analyze の残りの tail は CPU ではなく**順序**である —— span を決める 5 本は
   合計 0.16s しか働かず、0.8s 待っている。依存（`buildir`）は 0.34s で終わっている。
   `docs/PERF_TASKS_V3.md` の V1/V2 は「どの analyzer が重いか」の軸なので、
   **これはその表には出てこない**。着手前に `GUFF_DEBUG_CACHE=2` の
   critical path を取り直すこと（V1 が進むたびに顔ぶれが変わる）。
2. **CPU 表の上位から着手しないこと。** gocritic は tsdb の CPU 1 位だが 0.62s で終わる。
   `waited` と `[start..end]` を見てから決める。
3. missing 3 の移植（上記 4 に原因と実測がある）。SA5011 は callee の noreturn 性、
   SA4006 は guff-ssa のブロック融合、SA6001 は上流ソースの取得が先。
4. `replaceRecvType`（`subst.rs`）。優先度低のまま。**観測できる差分がまだ無い**ので、
   着手するなら先に「インスタンス化した generic interface のメソッドの型文字列」が
   実際にズレる入力を作ること。

---

### 2026-08-14（21 本目）— ゴールデンは「一台の記録」だった: linux 限定の fixture 2 本と、その裏に隠れていた SA4032 の欠陥 2 つ

PR #3 / #4 のあと、main で唯一赤かったのが smoke の `staticcheck-sa` ratchet である。
**同じコミット・同じ guff バイナリで、ホストだけ変えて測った**:

| ホスト | guff | golden | missing | extra | 判定 |
|---|---:|---:|---:|---:|---|
| darwin/arm64（開発機） | 257 | 259 | 3 | **1** | pass |
| linux/amd64（CI） | 259 | 259 | 3 | **3** | fail |

差の 2 件はちょうど **linux 限定の build constraint を持つ fixture** 2 本だった ——
`sa4019/bad.go`（`// +build linux` ×2）と `sa4032/bad.go`（`//go:build linux`）。
darwin では `go list` の段階で落ちるので誰も解析せず、**golden も darwin で記録されている**ので
そこにエントリが無い。linux では両方ビルド対象になり、guff の 2 件が extra として出る。

#### 1. ratchet を 3 に上げる案は却下した

それは「golden がプラットフォーム依存」という欠陥を baseline に焼き込む。
PR #3（runtime の Go が baseline の Go と違う）と PR #4（形の台帳が計測機の記録だった）で
直したのと**同じ型の欠陥**を、GOOS 軸で追認することになる。

#### 2. まず上流を測った ——「guff のバグ」ではなく「golden の欠落」だった

Docker の linux/arm64（Go 1.26.5・golangci-lint 2.12.2、CI と同じピン）で
`./compat/golden/run.sh --regen --case staticcheck-sa` を回すと **261 キー**（darwin は 259）:

```
> sa4019/bad/bad.go:4:1:staticcheck::SA4019: identical build constraints "linux" and "linux"
> sa4032/bad/bad.go:6:9:staticcheck::SA4032: ... runtime.GOOS will never equal "windows"
```

**上流も linux では両方報告する。** guff の 2 件は正しく、golden が短かった。
CI の extra 3 件のうち想定内なのは SA5005 の 1 件だけで、残り 2 件は
**ratchet を書いた人が darwin でしか測っていなかった**ことの帰結である。

#### 3. 軸そのものを消した ——「制約を消す」のではなく「どこでも同じに解決する制約にする」

`!windows` で足りるが、**`!plan9` にした**: リリース対象（linux/darwin × amd64/arm64）だけでなく
windows でも真になり、コストは同じである。検査の主題は変わらない ——
同一の `// +build` 行 2 本は依然 2 本だし、`!plan9` の下で `runtime.GOOS == "plan9"` は依然恒偽。

**全 81 ケースを linux で再生成して darwin の golden と突き合わせた**（platform 軸の全数調査）:

| 再生成した環境 | committed（darwin/arm64）との差 |
|---|---|
| linux/arm64（コンテナ・ネイティブ） | `staticcheck-sa` の上記 2 件のみ |
| linux/amd64（コンテナ・エミュレーション） | 同上 |

**GOARCH 軸は 1 件も無い。** `govet/framepointer` を §6 で golden から外してあるのが効いている。
（linux/amd64 では `revive` が 8 回とも自分と一致せず再生成できなかった。README の
「Upstream is not a function」の既知の非決定性で、この件とは無関係。）

#### 4. 比較できるようになった途端、SA4032 に本物の欠陥が 2 つ出た

fixture が linux 限定だった間、**この 2 つはどのゲートからも到達不能だった**。
両方 `honnef.co/go/tools` のパターン
`(BinaryExpr (Symbol "runtime.GOOS") op@(Or "==" "!=") lit@(BasicLit "STRING" _))` で裏を取った:

| 欠陥 | guff | 上流 |
|---|---|---|
| 報告位置 | 演算子（`op_pos`） | `report.Report(pass, node, …)` の `node` は `BinaryExpr` 全体＝**先頭トークン** |
| 被演算子 | どちら向きでも、**文字列定数**なら報告 | シンボルは**左**、値は **`BasicLit`** に固定 |

後者は実測でも確認した（`_ = "plan9" == runtime.GOOS` と `runtime.GOOS == someConst` は
golangci-lint 2.12.2 が **0 件**）。両方 `sa4032/ok.go` に対照として入れた。
**実利用では踏んでいない。** grafana と kubernetes は `runtime.GOOS == 名前付き定数` を書いている
（grafana は `windows = "windows"` を宣言して 4 回比較している）が、**どれも build constraint の無いファイル**で、
SA4032 は制約が空のファイルを見る前に返る。逆順は corpus に 1 件も無い。2 つとも潜在的な欠陥で、
**潜在のままだったのは、捕まえられる唯一の fixture が golden を記録した機械では見えなかったから**である。

#### 5. 予防: ゲートが自分の入力を検査するようにした

`compat/golden/platforms.py`。materialize した `.work/<name>/` を両ツールより先に走査し、
**すべてのファイルの「ビルド対象か否か」が 4 プラットフォームで一致しない限りケースを拒否する**。
不変条件は「build constraint を書くな」ではない —— SA4032 は build constraint **についての**
検査で、制約なしには test できない。「どこでも同じに解決すること」である。

* `linux` / `// +build linux` / `bar_linux.go` → 拒否
* `!plan9` / `unix` → 通る（4 つすべてで真）
* `custom` / `!nope` / `go1.24` → 通る（platform tag ではない。真偽**両方**を試して、
  どちらでも 4 つが一致することを確認する）
* `env` で GOOS/GOARCH を固定しているケース（`staticcheck-386` だけ）は軸が既に無いので、
  その 1 組だけを見る

入れた初回に **`govet` の `buildtag/ok/ok.go`（`//go:build linux`）を捕まえた。**
golden には元々エントリが無い（`ok` の fixture なので）が、
「整形式のヘッダ指令は flag されない」という**対照としての値が darwin では検証されていなかった**。
これも `!plan9` にした。単体テスト 24 本を `compat/tests/test_golden_platform.py` に置いた（smoke で走る）。

#### 6. 検証

| 環境 | golden gate | 単体 |
|---|---|---|
| darwin/arm64 | **81/81 OK**、`staticcheck-sa` は ratchet baseline（missing 3 / extra 1） | `compat/tests` 142 本 OK |
| linux/arm64（コンテナ） | **81/81 OK**、数字は darwin と完全一致 | `test_golden_platform` 24 本 OK |

`staticcheck-sa` / `govet` の golden を linux で再生成すると、**darwin で生成したものと byte 一致**する。

**未検証**: linux は GitHub runner そのものではなくコンテナ（arm64 はネイティブ、amd64 は
エミュレーション）。windows は support matrix に無いので誰も測っていない ——
`!plan9` は windows でも真なので、matrix を広げるときにこの 2 本は動かなくてよい。

**次にやること**

1. **guff 本体が `go list` のツールチェーン昇格に追従しない件**（PR #3 が CI 側だけ塞いだもの）。
   `GOTOOLCHAIN=auto`（Go の既定）で古い `go` が PATH にある利用者環境では、パッケージが
   丸ごと ill-typed になり **findings が既定で無警告に 0 になる**。golangci-lint は
   go/packages 経由なので影響を受けず、**上流との差分ゲートでも検出できない**（両者 0 件で
   「一致」に見える）。原因は `crates/guff-packages/src/golist.rs` の `go_root_from_path()` が
   `go env` のサブプロセス（実測 0.074s）を避けて `go` を実行しない設計。
   案: (a) `go list` が使った GOROOT と不一致なら警告 (b) それに従う。性能方針に触るので要判断。
2. missing 3 の移植（20 本目の「次にやること」3 のまま）。

### 2026-08-20 — hunt の 8 リポで 24 件を潰した: 3 件は linter ではなく土台の欠陥だった

**やったこと**

`compat/hunt.sh` の差分を 1 件ずつ上流ソースに突き合わせて 8 本の PR にした（#45–#52）。
7 本は個別 check の移植ずれだが、**3 件は解析基盤の欠陥**で、症状が出た linter は
たまたまそこを踏んだだけだった。

| PR | 何が壊れていたか | 効果 |
|---|---|---|
| #45 | `httpresponse` が「呼び先が net/http なら対象」で判定。`http.MaxBytesReader` + `defer r.Body.Close()`（リクエストボディを縛る定型）が全部 finding に。上流は**シグネチャが `(*http.Response, error)` であること**が唯一の条件。走査も BlockStmt/IfStmt だけで、`for` 本体と func literal を見ていなかった | FP 5 形・取りこぼし 2 形 |
| #46 | 変数を捕捉した func literal は `MakeClosure` になり、go/ssa は**この命令に位置を持たせない**。上流は `Reportf(ff)` に落として literal の `func` トークンを使うが、guff は位置なしの報告を黙って捨てていた。`guff_ssa::Function` に `decl_pos` を追加。逆に「捕捉なし func literal を `return` で追う」独自拡張は上流にない過剰報告で、削除 | jaeger 7 → 6 |
| #47 | **`Package.imports` はドライバのメタデータ stub**。fact producer の action はそこに型情報が無いとスキップされるので、**contextcheck の package fact が import 元に一切届かない**。`rewire_typed_imports` はパス前のスナップショットに対して解決していたので、import 先の import は stub のまま。加えて `filter_duplicate_packages` が `P [P.test]` を残して `P` を消すため、`Imports["P"]` は存在しない id を指していた。runner 側に「id → 型検査済みパッケージ」表を渡す形に変更（Package を作り直すと `Vec<File>` 丸ごと複製で jaeger peak RSS +36%） | jaeger 6 → 3 |
| #48 | **ラベル付き `break` が、抜けようとしているループの先頭へ飛んでいた**。5 つのループ builder が `set_label_loop_targets` を本体構築の**後**に呼ぶので、本体中の `break <label>` は解決に失敗し、`branch_stmt` のフォールバック（ラベルの goto ブロック）に落ちていた。go/ssa は本体の前に `label._break = done` を置く。CFG を読む全 analyzer に影響。ついでに SA4008 に「条件変数が本体で代入されるなら報告しない」を追加（上流の Phi/Sigma 判定の syntax 版） | gitea 8 → 6 |
| #49 | gocritic の stmt / stmtList / localDef walker は `f.Decls` のうち **`*ast.FuncDecl` にしか降りない**。パッケージレベル `var` の func literal は 27 checker から不可視。guff は file を flat に歩くので全部見えていた。`regexpSimplify` のエスケープ解除リストも 13 文字ちょうどに（`\ ` は含まれない） | argo-cd 7 → 5 |
| #50 | `thelper` が `synctest.Test(t, func(*testing.T))` の literal を helper 扱い（上流は `extractSubtestExp` と同様に filter）。**formatter の走査が nested module に入っていた** — argo-cd の `gitops-engine/` は自前の go.mod を持ち、`./...` は届かない。linter 側は `go list` 経由なので最初から正しかった | argo-cd 5 → 3 |
| #51 | `G122` が callback の path 引数を**名前でしか**追っていなかった。上流の `pathDependsOn` は sink 引数から BinOp / Convert / UnOp / **呼び出し引数**を遡るので `filepath.Clean(path)` も path。ただし**可変長引数の呼び出しで止まる**（go/ssa が引数を slice に詰めるため）ので `filepath.Join(path, x)` は finding ではない | coredns 4 → 3（P = R = 100%） |
| #52 | `usetesting` が「入れ子の関数は自分が訪問されたときに見る」としてネスト関数で走査を止めていた。上流は `*testing.T` を取る関数の**本体全体**を closure ごと見て、**囲む関数の名前**で報告する（引数を取らない closure は自分では対象にならないので、誰も中を見ていなかった） | gitea 6 → 5 |

**ゲート**

- golden case を 2 つ新設: `contextcheck`（isolate が `0 == 0` で何も保証していなかった §1 の 9 本のうちの 1 つ）と、既存 case への追加（govet の httpresponse 6 形、gosec の G122 3 形、gocritic のパッケージレベル literal 2 本と escape 2 本、staticcheck-sa の SA4008 ok 3 形）。
- isolate fixture を 4 本強化: contextcheck（0 → 3 findings）、wastedassign（ラベル break の両方向）、thelper（synctest と、synctest を騙る同名メソッド）、usetesting（`settings.yml` で `os-temp-dir` を on にし、closure / subtest / パッケージレベル literal の 3 形）。
- `crates/guff-ssa/tests/range_break_test.rs` に CFG 直接の pin: 無限ラベルループの exit には predecessor があり、それはラベル自身のブロックではない。

**hunt の現在地（2026-08-20, #52 込み）**

| リポ | guff | golangci | guff-only | golangci-only |
|---|---:|---:|---|---|
| prometheus v3.14.0 | 20 | 20 | 0 | 0 |
| coredns v1.14.6 | 3 | 3 | 0 | 0 |
| argo-cd v3.5.1 | 3 | 3 | 0 | 0 |
| atlas v1.3.0 | 2 | 0 | gosec G101 ×2 | 0 |
| jaeger v2.20.0 | 3 | 0 | revive ×2（容認済み ratchet）/ contextcheck ×1 | 0 |
| gitea v1.27.2 | 5 | 0 | unparam ×4 / staticcheck ×1 | 0 |
| thanos v0.42.4 | 432 | 434 | staticcheck ×1 | unparam ×2 / staticcheck ×1 |
| dapr v1.18.3 | 1465 | 1364 | 108 件 | gosec ×6 / prealloc ×1 |

**次にやること**

1. **`unparam` の未実装 2 系統**（gitea ×4、thanos ×2）。`crates/guff-style/src/unparam.rs` は
   「未使用パラメータ」だけの AST 近似で、ヘッダに DEFERRED と書いてある通り
   **定数パラメータ（`x always receives "y"`）と未使用/定数 result（`result 1 (T) is always 0`）を
   実装していない**。上流は SSA + callgraph。単発の移植ではなく一区切りの仕事。
   再現: `cd corpus/cache/gitea && golangci-lint run --no-config --default=none -E unparam ./modules/...`
   （4 件出る。guff は 0 件）。
2. **gosec G101 の entropy**（atlas ×2）。上流は **zxcvbn**（辞書ベース）で
   `isHighEntropyString` を判定し、guff は Shannon エントロピー × 長さの近似。
   `TypeTimestampWTZ = "timestamp with time zone"` は名前に `pw` を含むため name pattern に当たり、
   近似では per-char 3.125 ≥ 3.0 で通ってしまう。zxcvbn の移植か、別の判定が要る。
3. **contextcheck の残り 1 件**（jaeger `internal/storage/v1/elasticsearch/factory.go:86`）。
   #47 で package fact は届くようになったが、この 1 件は**パッケージ単体で走らせても出ない**ので
   別の欠陥。`NewFactoryBase$1->Close->Close` の連鎖。
4. **SA4006**（gitea `modules/setting/config_env.go:128`）。上流が出して guff が出さない。
   最小化を試したが `keyValue := envValue` + 条件付き上書きの単純形では両者とも黙るので、
   トリガはもう少し複雑（`staticcheck-sa` の ratchet は missing 3 / extra 1 のまま）。

---

### 2026-08-20（続き）— 同じセッションの後半 8 本（#54–#61）と、比較対象のバージョンという落とし穴

**追加で潰したもの**

| PR | 何が壊れていたか | 効果 |
|---|---|---|
| #54 | `contextcheck` の `func_type_pkg` が `Function.pkg` しか見ていなかった。**import 先のために on-demand で作られた関数には SSA package が無い**ので、パッケージを跨いだ callee は「fact を誰も持っていない関数」に見えていた。宣言元 object から辿るフォールバックを追加。golden case は 2 パッケージ構成（fact がパッケージ間を渡ることは 1 パッケージでは再現できない） | jaeger 3 → 2（残り 2 は容認済み revive ratchet） |
| #55 | `protogetter` が「getter が値を返すのにポインタ欄へ代入する」形（`Schedule: job.Schedule` で `*string` ← `GetSchedule() string`）を filter していなかった。上流は `hasPointerKeyWithoutPointerGetter`。guff の checker は複合リテラルのキーに型を記録しないので、リテラル自身の struct 型から欄を引く | dapr 108 → 92 |
| #56 | `spancheck` の報告位置。上流は代入文と、CFG が辿り着いた `return` を指す。guff は右辺の call と**閉じ括弧**を指していた。加えて **span とその return の間に分岐があると上流は何も報告しない**（2 つのメッセージは 1 つの `if ret != nil` の下）。6 形を実測して構文で再現 | dapr 92 → 82 |
| #57 | `unused` が**ジェネリックなインターフェース**のメソッド名も「実装したら使用」の集合に入れていた。staticcheck は型パラメータ付きインターフェースには実装エッジを張らないので、dapr の 10 個の streamer の 40 メソッドは全部 finding | dapr 82 → 43 |
| #58 | `bodyclose` が「左辺が `*http.Response` なら追跡」だった。上流は **call の結果**だけを追う。チャネル受信・map・slice・コピー・フィールドは対象外。報告位置も call の `(` へ | dapr 43 → 39 |
| #59 | `prealloc` の容量式が `go/printer` の空白規則に従っていなかった（`len(a)/2 + len(b)`）。`Cap` 側は従っていたが、**leaf は構築時に depth 1 で 1 度だけ描画**していたので、後から低優先度の演算子の下に入ると空白が残った | 同じ finding が「取りこぼし 1 + 誤検出 1」から「一致」へ |
| #60 | gocritic のコメント walker が `astwalk.visitCommentGroups`（**ブロックコメントは単独のグループにする**）を持っていなかった。`commentFormatting` は「先頭が `/*` なら return」なので、行コメントの直後に置かれたブロックコメントが finding になっていた | dapr 39 → 38 |
| #61 | `recvcheck` の組み込み除外リストが**逆**だった。v0.2.0 は Marshal 側、v0.3.0 は Unmarshal 側で、golangci-lint 2.12.2 が pin しているのは **v0.2.0** | dapr 38 → 33 |

**比較対象のバージョンは `git show v2.12.2:go.mod` で確認する**

`recvcheck` はこれで一度間違えた。手元の golangci-lint チェックアウトは `v2.12.2-65-g…` で、
その `go.mod` は **リリース版バイナリより新しい依存**を指している。module cache に複数バージョンが
あるときに `ls | tail -1` で選ぶのはもっと危険で、`go-critic` は 0.14.4 を読んでいた（該当ファイルは
0.14.3 と同一だったので結果は無事）、`x/tools` は 0.46.0 を読んでいた（`httpresponse` の差分は
`slices.Backward` への書き換えだけ）。`recvcheck` だけは v0.3.0 と v0.2.0 で**除外リストが入れ替わって
いた**ので、読んだ版のまま実装すると全部逆になる。

2026-08-20 時点の pin（`git show v2.12.2:go.mod`）:
`gosec v2.26.1` / `go-critic v0.14.3` / `contextcheck v1.1.6` / `thelper v0.7.1` /
`usetesting v0.5.0` / `protogetter v0.3.20` / `spancheck v0.6.5` / `prealloc v1.1.0` /
`recvcheck v0.2.0` / `honnef.co/go/tools v0.7.0` / `x/tools v0.44.0`。

**hunt の最終状態（2026-08-20, #61 込み）**

| リポ | guff | golangci | guff-only | golangci-only |
|---|---:|---:|---|---|
| prometheus v3.14.0 | 20 | 20 | 0 | 0 |
| coredns v1.14.6 | 3 | 3 | 0 | 0 |
| argo-cd v3.5.1 | 3 | 3 | 0 | 0 |
| jaeger v2.20.0 | 2 | 0 | revive ×2（容認済み ratchet） | 0 |
| atlas v1.3.0 | 2 | 0 | gosec G101 ×2 | 0 |
| gitea v1.27.2 | 5 | 0 | unparam ×4 / SA4006 ×1 | 0 |
| thanos v0.42.4 | 432 | 434 | staticcheck ×1 | unparam ×2 / staticcheck ×1 |
| dapr v1.18.3 | 1391 | 1364 | 33 件 | gosec ×6 |

セッション開始時は prometheus 0 / coredns 1 / atlas 2 / jaeger 7 / argo-cd 7 / gitea 8 /
thanos 3 / dapr 111 だった。**3 リポが完全一致、jaeger は容認済み ratchet だけ**になった。

**次にやること（更新）**

上の 1〜2 と 4 はそのまま。3（contextcheck）は #54 で解消。加えて:

5. **dapr の残り 33**。内訳は `nolint:` カスケード 20（gosec 10 / bodyclose 5 / testifylint 2 /
   gocritic 1 / unused 1 / usetesting 1）と直接 13。gosec カスケードの半分は
   **G201/G202（SQL 文字列連結）が未実装**（`crates/guff-style/src/gosec.rs` の DEFERRED 行）で、
   残り半分は G101 の entropy（上の 2 と同じ原因、ただし**取りこぼし側**: `AppAPITokenEnvVar =
   "APP_API_TOKEN"` を上流は報告し guff はしない）。G101 は誤検出と取りこぼしの両方を出しており、
   zxcvbn を入れると atlas 2 + dapr 4 + カスケード 6 が一度に動く。
6. **bodyclose のカスケード 5**（dapr `tests/integration/suite/actors/http/ttl.go` ほか）。
   #58 は誤検出側を直したが、`resp, err = client.Do(req)` を `t.Run` のクロージャ内で書く形は
   まだ取りこぼす。

   **一度やって外した仮説を書き残す**（2026-08-20）。小さく作った 3 形では、上流は
   「**クロージャが書き込む変数**に代入された response は、閉じていても全部報告する」
   ように見える。go/ssa がその変数をヒープに退避するので `Referrers()` が cell を越えられない、
   という説明も付く。実際:

   | 形 | golangci-lint 2.12.2 | guff（当時） |
   |---|---|---|
   | 外で開いて閉じ、クロージャが同じ変数に再代入 | 外側も内側も報告 | どちらも黙る |
   | クロージャが**読むだけ** | 黙る | 黙る |
   | クロージャ内で再代入し、そこで閉じる | それでも報告 | 黙る |

   この規則を実装すると 3 形は一致するが、**dapr で bodyclose が 259 件に膨らむ**
   （名前一致で 268、型検査 object 一致にしても 259）。つまり上流の不精確さは
   「クロージャが書く変数」よりずっと狭い。**この規則は間違い**なので入れていない。
   次にやるなら `timakin/bodyclose` の SSA 解析（`pkg/analyzer/analyzer.go` の
   `Referrers()` の辿り方）を読んで、5 件と 259 件を分けている条件を先に特定すること。
   再現用の 5 形は `scratchpad` ではなく最小モジュールとして作り直せば足りる。

---

### 2026-08-20（続き 3）— corpus が 14 リポになった分の初回掃除（#65–#68）と、SA5011 の「Exit ブロック」という土台差

**前提の変化**: `compat/repos.txt` に go-redis / nats-server / rclone / restic / cli / traefik が
加わっていて、hunt は 14 リポになっている。前半の 8 リポは片付いていたので、
このセッションはほぼ新しい 6 リポの初回掃除になった。

| PR | 何が壊れていたか | 効果 |
|---|---|---|
| #65 | **ST1023 と QF1011 が別々の近似だった**。上流は `sharedcheck.RedundantTypeInDeclarationChecker` を `flagHelpfulTypes` 違いで 2 回呼ぶ**1 つの関数**で、右辺を**文脈から切り離して型検査し直し**（`types.CheckExpr`）、untyped なら `types.Default` と宣言型を**同一性**で比べる。3 つ間違えていた: ① untyped rune の default は alias の `rune` であって `int32` ではないので `var v int32 = 'a'` は型を落とせない（guff の arena は alias を畳むので、宣言がどう綴っているかから復元する）② shift は左オペランドの型を取るので `var n uint = 1 << uint(x)` も落とせない ③ ST1023 は**typed 右辺**の `int64(k)*10` / `5*time.Second` / `b1 && b2` / `<-ch` / 名前付き typed 定数を全部取りこぼしていた（「名前付き定数の型は読み手を助ける」という除外は untyped 枝の**中**にしかない） | thanos guff-only 3 → 0（P 100.0%） |
| #66 | printf の `%[2]*[1]s`: **index はそれを吸収した位置に属する**。幅の `*` が `[2]` を吸い、続く `[1]` は verb のもの。guff は directive ごとに index を 1 つしか持たず、しかも star より前に適用していたので幅と値が入れ替わり、rclone の `SizeStringField` を「`%s` に int」と報告していた。読みながら同じ関数の 3 つも直した: `*` のオペランドを型検査していなかった（`uses non-int … as argument of *`）、上流は**書式文字列ごとに 1 件**しか報告しない（しかも `fmtstr.Parse` が全体を先に読むので malformed があれば**それだけ**）、`[999999999999]` は「大きい」ではなく `ParseInt(…,10,32)` が弾く**不正な index**、`%[3d` は `is missing closing ]`、`%[1]` は `is missing verb at end of string` | rclone guff-only 5 → 3 |
| #67 | **defer する関数の名前付き result は lift しない**（go/ssa `liftAlloc` 冒頭の `fn.Recover != nil`、honnef は `fn.hasDefer`）。deferred call が result に代入し得るからセルのまま残す必要がある。lift すると各代入が register 定義になり、「上書き前に読まれない値」に見える。SA4006 が rclone の `startRc`（`err` が名前付き result で、`defer serveMu.Unlock()` の前に書かれる）を撃っていた | rclone 3 → 2 / dapr 26 → 23 |
| #68 | SA5011 の「この abort は本物か」を**囲む関数の引数**で見ていた（interface を取れば TB＝soft）。`func(k, v any) bool` コールバックは interface を取るので、中の `t.Fatalf` が全部 soft 扱いになる。決めるのは**レシーバ**（interface invoke は `method` が付き、static call は付かない）。加えて短絡形 `if a == nil \|\| b == nil { t.Fatalf(…) }` は abort がどちらの条件からも届くので dominance では届かない。名前だけでも足りない: ローカル型の `Fatal` は空実装でも noreturn ではなく、**上流は素通しで報告する**（`sa5011/ok.go` の `fataler`）。`os.Exit` / `runtime.Goexit` / `log.Fatal*` / `testing` の abort メソッド（具象レシーバ）だけを hard abort とした | nats-server 6 → 4 |

**測ったが入れなかったもの — SA5011 の「Exit ブロック」**

`if p == nil { return }` の**上**にある deref を、上流は報告しない。guff は報告する。
これは σ ノードの話ではなく、**honnef IR が全 return を単一の `f.Exit` へ Jump で集める**ことの
帰結だった。`jumpThreading` が「a→b→c で b がただの Jump なら a→c」を適用し、その結果
`a.Succs[0] == a.Succs[1]` になった `If` は **Jump に置き換えられる**（`go/ir/blockopt.go:94`）。
つまり `if p == nil { return }` が関数の末尾にあると **`If` 自体が消え、`maybeNil` に何も登録されない**。
go/ssa 系の guff は `Return` を各ブロックに直接置くので b は「ただの Jump」にならず、`If` が残る。

実測（golangci-lint 2.12.2、`checks: ["SA5011"]`）:

| 形 | 上流 | 理由 |
|---|---|---|
| `fmt.Println(*x)` → `if x == nil { return }`（結果なし関数の末尾） | 黙る | `If` が Jump に畳まれる |
| 同上だが `return 0`（結果あり） | 報告 | return ブロックが Store を含むので Jump 単体にならない |
| 同上で `if` の後に文が続く | 報告 | 偽側の後続が Exit ではない |
| `if x == nil { panic(…) }` | 報告 | panic は Jump ではない |
| `if x == nil { t.Errorf(…) }` → deref | 報告 | 分岐が `x` を読まないので σ が刈られ、join の φ は自明に畳まれる |
| `if x == nil { t.Errorf("%v", x) }` → deref | 黙る | σ が生き残り φ が畳まれない |

guff 側で後者 2 行（Errorf の有無）を再現しようとして
「非 nil 側後続が check からしか入れないこと」を dominance guard に足したら、
**nats-server `accounts.go` の複合条件（`if a == nil && b == nil` / `(a == nil && b != nil) || …`）で
偽陽性が 9 件出た**。そちらは分岐ブロック自身が `a` を読むので σ が生き残る側で、
「pred のパスが値を読むか」を条件に足しても 9 件は消えなかった。**この方向は入れていない**。
正しい直し方は当て木の積み増しではなく、**σ/φ の生存を 1 つの値について実際にシミュレートする**か、
IR に Exit ブロックを入れることのどちらか。`staticcheck-sa` の ratchet は missing 3 / extra 1 のまま。

**ゲート**

- golden: `staticcheck-st` / `staticcheck-qf` に `isolated.go`（38 宣言、上流と 38/38 一致を確認してから regen）、
  `govet` に `printf/indexes.go`（23 呼び出し、114/114 で ratchet なし）、
  `staticcheck-sa` に `sa5011/testing_abort.go`（両形とも**どちらも報告しない**ので、退行は extra として出る）と
  `sa4006/ok.go` への追加。
- `crates/guff-ssa/tests/lift_named_results_test.rs`: 逆アセンブルを直接読み、defer する関数では
  名前付き result が `local error (err)` を保ち、defer しない関数では消えることを固定（修正前に落ちることも確認）。

**hunt の現在地（2026-08-20, #68 込み）**

| リポ | guff | golangci | guff-only |
|---|---:|---:|---|
| prometheus / coredns / argo-cd / restic / go-redis | — | — | 0 |
| thanos v0.42.4 | 431 | 434 | 0（golangci-only が unparam ×2 / staticcheck ×1） |
| cli | 4 | 3 | nolintlint ×1（bodyclose の recall 由来） |
| traefik | 70 | 69 | nilerr ×1 |
| rclone | 5 | 3 | unused ×2 / staticcheck ×1 |
| nats-server | 5 | 1 | staticcheck ×1 / unused ×2 / ineffassign ×1 |
| atlas v1.3.0 | 2 | 0 | gosec G101 ×2 |
| jaeger v2.20.0 | 2 | 0 | revive ×2（容認済み ratchet） |
| gitea v1.27.2 | 5 | 0 | nolintlint ×5（unparam ×4 と SA4006 ×1 の recall が全部 nolintlint に化けている） |
| dapr v1.18.3 | 1381 | 1364 | 23 |

**次にやること**

1. **gitea の 5 件は全部 nolintlint のカスケード**である。`//nolint:unparam` が 4 本、
   `//nolint:staticcheck` が 1 本あり、guff が本体の finding を出さないので「この directive は不要」と
   報告している。つまり **unparam の 2 系統（#前セッションの 1.）を実装すると gitea は 4 件消える**。
2. **gitea `config_env.go:128` の正体が分かった**: `SA4006: this value of keyValue is never used` で、
   **上流の偽陽性**（gitea 側のコメントも "false positive" と書いている）。
   再現は `//nolint:staticcheck` を外して `golangci-lint run ./modules/setting/...`。
   前セッションで「最小化できない」と書いたのは、最小形だと上流も黙るから。
   bug-for-bug 互換の対象なので、σ/φ を実際に持つ必要がある側の話。
3. **rclone の unused ×2** は build tag 付きファイル（`systemd_unsupported.go`）が絡む。
4. **nats-server の ineffassign ×1**（`jetstream_cluster.go:10730`）と SA5011 ×1（`jetstream_cluster_3_test.go:8707`）。

---

### 2026-08-20（続き 4）— 残り 6 リポの掃除（#69–#73）と、SrcFuncs が「宣言から辿れる関数だけ」である件

| PR | 何が壊れていたか | 効果 |
|---|---|---|
| #69 | **`return f()`（多値の末尾呼び出し）が tuple をそのまま返していた**。go/ssa は `len(s.Results) == 1 && sig.Results().Len() > 1` の枝で**要素を Extract して返す**。`Return.results` を読む側からは、宣言が複数の結果を持つのに値が 1 つ（tuple 型）しかないように見える。nilerr は「error 型の結果が全部 const nil か」を見るので、**error 型の結果が 1 つも無い** → traefik の `return r.rw.Write(p)` が「握り潰し」に化けた | traefik P = R = 100% |
| #70 | **`//lint:ignore U1000` を読んでいなかった**。honnef の `unused` は自前の directive を読む（`unused.go` の "all objects annotated with a //lint:ignore U1000 are considered used"）。上流は**コメントが付いているノードの行**をキーにする（`ast.NewCommentMap`）ので、行末コメントならその行、doc コメントなら下の宣言の行。行末コメントから下の宣言まで伸ばすと 1 行下を黙らせてしまうので、コメントの左に何かあるかで区別する。共有ロードは `PARSE_COMMENTS` 無しなので、ソースから読み直す（`//lint:` を含まないファイルは部分文字列検索で弾く） | rclone P = R = 100% |
| #71 | ループ 3 件 + パーサ 1 件。① `rangeint` の**報告位置が init 文ではなく `for`**（4 桁ズレ。column を見るのは golden tier だけで、`rangeint` の case が無かった）② `for i = 0` 綴りで**ループの後に i を読むなら報告しない**（range ループは `limit-1` を残す）—— guff はスタブで常に false ③ `intrange` の `i <= n` は**リテラル限定**（fix が「リテラル+1」を書くため）で、guff は定数畳み込みをしていた ④ 新 golden case が見つけた: **`RangeStmt.range_` が常に `NO_POS`**。go/parser は `Range: as.Rhs[0].Pos()` を入れる。key の無い `for range len(s)` は他に位置が無いので、intrange の finding が位置なしで捨てられていた | dapr 23 → 21 |
| #72 | ① **S1031 が else 付きの nil チェックを報告**（パターンの末尾 `nil` は else 枝）② **gocritic assignOp が getter 呼び出しを消した**。`.Where(m["x"].Pure)` で、ruleguard の `isPure` が受け入れる呼び出しは型変換だけ ③ **testifylint が祖先スタックを 1 つずらして読んでいた**。上流のスタックは末尾が呼び出し自身（`stack[len-2]` が親）だが、`preorder_stack` は祖先だけを渡す。結果 `if assert.NoError(t, err) {` が全部 finding になり、`find_root_if` も同じズレで `if` を見失うので**その本体の assertion まで**報告していた（4 つのヘルパが同じ読み方だった） | dapr 21 → 16 |
| #73 | **`buildssa.SrcFuncs` は `file.Decls` の `*ast.FuncDecl` から辿れる関数だけ**。パッケージ初期化関数には宣言が無いので、**パッケージレベル `var` の中の func literal は buildssa 系のどの analyzer からも見えない**。guff の gosec は `Package.members`（`init` を含む）から始めていたので、dapr の `pluggable.go` の `uint64(strconv.Atoi(…))` 2 つを G115 として報告していた。honnef の `buildir` は逆に `irpkg.Functions` から始めるので初期化関数を**含む** —— だから直したのは gosec 側のリストだけ | dapr 16 → 15 |

**測って分かった、まだ直していないもの**

1. **`unused` は「型が test ファイルで宣言され、かつ使われている」とき、そのメソッドを報告しない場合がある**（nats-server ×2）。
   `func (c *cluster) zzWhatever()` を足しても上流は黙るが、`func (c *client) zzWhatever()`（`client` は非 test ファイル）や
   `zzUsedType`（test ファイルで宣言し test で使う、その場で作った型）だと報告する。honnef の規則表（`unused.go` 冒頭）に
   該当するものが見当たらず、`colorAndQuieten` の `owns` 伝播も**メソッドには張られていない**（`g.see(obj, nil)`）。
   nats-server の `cluster` は約 100 メソッドを持つ巨大な test ヘルパ型。再現は `corpus/cache/nats-server` に
   `func (c *cluster) zzA() {}` だけのファイルを置いて `golangci-lint run --no-config --default=none -E unused ./server/...`。
2. ~~**gosec G101 の zxcvbn**~~ **→ #74 で移植した（下記）**。今回、**両方向で効いている**ことが分かった: dapr で guff-only の
   G101 が 4 件（偽陽性）、**上流だけが出す G101 が 3 件**（`//nolint:gosec` が付いているので nolintlint のカスケードとして現れる）、
   atlas で 2 件。合計 9 件で、corpus に残る単一項目としては最大。
   `isHighEntropyString` は `len(str) >= 8` を満たす文字列の**先頭 16 文字**について
   `zxcvbn.PasswordStrength(s, []string{}).Entropy` を取り、`Entropy >= 80 || (Entropy >= 40 && Entropy/len >= 3.0)`。
   16 文字なら実質「エントロピー 48 以上」。zxcvbn は辞書（英単語・人名・パスワード・キーボード配列）を持つので、
   移植は**データを含む一区切りの仕事**になる。
3. **nats-server の ineffassign ×1**（`jetstream_cluster.go:10730`）。`goto RETRY` がラベルより後ろに 3 つあり、
   `sreq = nil` はその経路で再び読まれる。最小化を 3 形試したが（後方 goto / select 内 goto / switch 内 goto）
   どれも両者一致するので、トリガはこの関数の別の要素。

**この時点の hunt（14 リポ）**

| リポ | guff-only |
|---|---|
| prometheus / coredns / argo-cd / restic / go-redis / traefik / rclone / thanos | 0 |
| cli | 1（nolintlint、bodyclose の recall 由来） |
| jaeger | 2（容認済み revive ratchet） |
| atlas | 2（gosec G101） |
| nats-server | 4（unused ×2 / SA5011 ×1 / ineffassign ×1） |
| gitea | 5（全部 nolintlint。unparam ×4 と SA4006 ×1 の recall） |
| dapr | 15（nolintlint ×11 / gosec G101 ×4） |

---

### 2026-08-20（続き 5）— zxcvbn を移植した（#74）: G101 の判定は「長さ」でも「文字種」でもなく辞書だった

**やったこと**

`github.com/ccojocar/zxcvbn-go` v1.0.4 を Rust に移植し（`crates/guff-style/src/zxcvbn/`、
約 1,000 行 + データ 688KB）、G101 の `isHighEntropyString` をそこに繋いだ。
guff はそれまで Shannon エントロピー × 長さの近似で代用していた。

上流の判定は
`len(str) >= 8` を満たす文字列の**先頭 16 バイト**について
`Entropy >= 80 || (Entropy >= 40 && Entropy/len >= 3.0)`。
zxcvbn のエントロピーは**辞書ベース**なので、英単語で綴った文字列はどれだけ長くても低く、
そうでない文字列は半分の長さでも高い。近似では原理的に分けられない:

| 文字列（先頭 16 バイト） | エントロピー | 上流 |
|---|---:|---|
| `secretStoreName` | 26.148 | 出さない |
| `mockSecretStore` | 28.713 | 出さない |
| `local-secret-sto` | 36.087 | 出さない |
| `timestamp with t` | 39.777 | 出さない |
| `DAPR_API_TOKEN` | 55.449 | **出す** |
| `/var/run/dapr/cr` | 77.833 | **出す** |

**移植で写した「バグ」**（どれも実際の判定を動かす）

- `l33tMatch` は各 match の**コピー**に extra l33t エントロピーを足して捨てるので、
  l33t 由来の match は素の辞書エントロピーのまま。
- `endUpperRx` は `^[^A-Z]+[A-Z]$'` —— アンカーの後ろに `'` があるので**何にもマッチしない**。
- `CalculateAvgDegree` は隣接エントリの**文字数**を数える（`"2@"` は 2）。空エントリも母数に入る。
- date match の `J` は end-exclusive（他の matcher は inclusive）。
  `dateWithoutSepMatchHelper` の token はパスワード全体。

**検証**

corpus 7 リポの文字列リテラルから 20,023 本を抜き、Go 実装と Rust 実装のエントロピーを突き合わせた
（`scratchpad/zx/`: Go 側は 40 行のドライバ、Rust 側は `ZXCVBN_IN`/`ZXCVBN_OUT` を読む
`differential_dump` テスト）。**20,019 本が 0.001 以内で一致**。
残る 4 本はすべて先頭 16 バイトの途中でマルチバイト文字が切れるもので、
Go はバイトで切って壊れた文字を残し、Rust は文字境界まで戻る。
**4 本とも高/低の判定は一致**しているので、差はエントロピー値だけ。

**効果**

| リポ | before | after |
|---|---|---|
| atlas | guff-only 2（G101） | **0（P = R = 100%）** |
| dapr | guff-only 15 | **9**（G101 の偽陽性 4 と、`//nolint:gosec` のカスケード 2 が消えた。P 98.9% → 99.3%） |

ゲートは `gosec/entropy.go`（報告される 4 本と、報告されない 6 本を並べた fixture）を golden tier に。
辞書と隣接グラフを丸ごと持ち込んだので `THIRD_PARTY_LICENSES.md` に MIT として記載した。

---

### 2026-08-20（続き 6）— bodyclose の「値が無い」経路（#75）と unparam の残り 3 系統（#76）

**#75 — bodyclose**

`isopen` は `ssa.Call` の referrer から `*http.Response` を運ぶ `ssa.Extract` を探し、
その先の switch の各枝は「閉じられていることの証明」になっている。
**証明する材料が無いときは `return true`（＝報告）** で終わる。3 つ直した:

| 形 | 上流 | guff（修正前） |
|---|---|---|
| `_, err = client.Do(req)` | 報告（Extract が無い） | 黙る（blank を素通し） |
| `return StatusScopesResponder(...)`（`func(*http.Request) (*http.Response, error)` を返す呼び出し） | 報告（`getReqCall` は**型文字列の部分一致**） | 黙る |
| `http.Get(...)` の裸の文 | col = `(` の位置 | col = callee の先頭（4 桁ズレ） |

逆方向も 2 つ: **`make` と `new` は go/ssa では Call ではない**（`MakeChan`/`MakeMap`/`MakeSlice`/`Alloc`）ので
`resp := new(http.Response)` も `make(chan *http.Response)` も対象外。部分一致ルールを入れた瞬間に
両方が finding になったので、同時に除外した。

`bodyclose/ok.go` は「blank は finding ではない」と書いてあったが**測っていなかった**。実際は finding なので
`bad.go` に移した。golden case を新設（isolate は column を正規化するうえ、これらの形の case が無かった）。

**#76 — unparam**

`unparam.rs` は「未使用パラメータ」だけで、ヘッダにも DEFERRED と書いてあった。
gitea の 4 件（nolintlint のカスケードとして見えていた）は残り 3 系統:

- **`result N is always X`** — 全 `return` が同じ定数を返す。ただし「return が 1 つで、定数が untyped nil でない」場合は
  上流が「偽陽性が多すぎる」として除外。
- **`result N is never used`** — どの呼び出し側も結果 N を読まず、無視している呼び出しが 2 つ以上ある。
  `error` 型の結果は errcheck の仕事なので対象外。
- **`param always receives X`** — 呼び出しが 4 つ以上あり全部同じ定数を渡す。本体で使っていても報告し、
  **全呼び出しの綴りが一致すればソースの綴りで**（`statusOK (200)`）。

**除外条件のほうが本体と同じくらい効く**（どれも corpus で実際に踏んだ）:

- `dummyImpl`: 最初のブロックが定数を返すだけ、または harmless な呼び出し（`\berrors\b` を含む）だけなら**関数ごとスキップ**。
  `func f() (int, error) { return 0, nil }` は "result 1 is always nil" ではない。
- `resultsRequiredBy`: `return f(...)` は f の結果を固定する。ただし**呼び出しが return の一部であるときだけ** ——
  `a, b := f(); return a, b` は `prev.Pos() < parent.Pos()` で弾かれる。gitea の `getStorageSectionByType` が
  finding のままなのはこのおかげ。
- `multipleImpls`: パッケージ**ディレクトリ**内に同名の宣言が 2 つ（＝ build tag で切り替わる別実装）ならスキップ。
  ローダが渡してくれないファイルまで自分で `read_dir` して数える。thanos の `materializeForUnmarshal` がこれ。
- 可変長引数: go/ssa は slice に詰めるので、可変長パラメータが定数になることは無い。

**guff 固有の補正が 2 つ**（どちらも他の x/tools 系 analyzer 移植にも効く）:

1. **guff の SSA は `DebugRef` 命令を持つ**。`buildssa` は `ssa.BuilderMode(0)` なので上流のグラフには無い。
   referrer を歩く処理と `dummyImpl` の走査は DebugRef を飛ばさないと、全呼び出し側が「タプル全体の実使用」に見える。
2. **ゼロ値の `Const` は `val: None` のまま**（go/ssa は `soleTypeKind` で `0`/`false`/`""` に正規化する）。
   メッセージが `nil` になってしまうので読み出し側で正規化する。

**土台の修正（#76 に同梱）: range-over-func の中の `return` が結果を捨てていた**

go/ssa の `returnStmt` は**まず結果を格納**してから jump 変数を立てて `return false` する。
yield クロージャには named result が無いので、格納先は
`fn.lookup(fn.returnVars[i], false)` ——「囲む関数の結果セルを自由変数として引く」。
guff は「自分に named result があるときだけ」格納していたので、**yield クロージャの `return` の値は消えていた**。
外側の関数の `Return` には自分の末尾 `return` しか残らず、`Return.results` を読む全ての利用者が
「3 つ return があるのに 1 つしか無い関数」を見ることになる。traefik の `lookupMiInstances` が
"result 1 (error) is always nil" になったのはこれ（`for … range chunkIDs(…)` の中の
`return nil, fmt.Errorf(…)` が 2 つとも見えていなかった）。`Function` に `return_vars` を追加した。

**効果**: gitea guff-only 5 → 1（残りは SA4006 の**上流の偽陽性**）、cli 1 → 0、
traefik / thanos は 0 のまま（一度 2 件ずつ増やしてから、上記の除外条件で戻した）。

### 2026-08-21（続き 7）— unused は「参照されたか」ではなく「根から辿り着くか」、そして go1.26 の `new(expr)`

**#79 — unused の到達可能性**

dapr の `//nolint:unused` が 1 件だけ「未使用のディレクティブ」に見えていた。剥がして
単独で走らせると差は明白だった:

```go
func (w *workflowAccessPolicies) recompileAll() { … }   // 上流だけが報告
func (w *workflowAccessPolicies) update(…)      { …; w.recompileAll() }
func (w *workflowAccessPolicies) delete(…)      { …; w.recompileAll() }
```

`update` と `delete` を呼ぶものは無い。honnef の `unused` は
**グラフを根（`NodeID(0)`）から色塗りする**（`unused.go` の `color`）ので、
死んだ関数の中に書かれた呼び出しは対象を生かさない —— 3 つとも報告される。
guff は `for obj in info.uses.values() { used.insert(obj) }` という
**参照カウント**だったので、2 つしか出せず、3 つ目の nolint をカスケードで潰していた。

移植したのは honnef の `g.use(used, by)` の `by` にあたる**帰属**:

| 宣言 | 所有者（`by`） |
|---|---|
| `FuncDecl`（レシーバ・シグネチャ・本体・入れ子の `FuncLit` すべて） | その関数オブジェクト |
| `TypeSpec` | その型オブジェクト |
| `ValueSpec` | そのスペックが宣言する**全ての名前**（上流は `names[i]`↔`values[i]`。緩い側に倒してある） |

根は「エクスポートされた宣言 / `init` / `main` パッケージの `main`」に加えて
**生成ファイルの全オブジェクト**（`GeneratedIsUsed` は既定 on ＝ `g.use(obj, nil)`）。
今までは生成ファイルを候補集めごとスキップしていたが、到達可能性の下では
「候補でない」と「根である」は別物で、前者だと**そこから伸びる辺が全部切れる**。

歩き切れなかった `*ast.Ident` は**根扱いにフォールバック**する。辺を落とすと偽陽性、
根を増やしても報告漏れにしかならない、という非対称性がある。

嵌まりどころが 2 つあった:

1. **`used` を `roots` で初期化してはいけない**。BFS の中で `if !used.insert(obj) { continue }` と
   書いていたので、根は最初の pop で「もう入っている」と判定され、**根から出る辺が一本も辿られなかった**。
   結果、`func New() *t` があるのに `type t is unused` という偽陽性が出た。`used` は空から始める。
2. **インスタンス化メソッドの名前キー規則が全部を根に戻していた**。
   `used_methods`（`(レシーバ型名, メソッド名)`）はジェネリックの実体化コピーが
   宣言と別 ObjectId になる件のための逃げ道なのに、無条件だったので
   普通のメソッド呼び出し（宣言と同じ ObjectId）まで拾い、`recompileAll` を生かし続けていた。
   **このパッケージの宣言でない使用にだけ**効かせる。

3. **不動点ループが止まらなかった。** const グループ規則が「既に到達済み」のオブジェクトも
   queue に積み直していたので、`queue.is_empty() && used.len() == before` という停止条件が
   **永遠に成立しない**（積んだ直後に空でないと判定 → 次の周回で何も増えない → また積む）。
   `unused_const_group_*` の 2 本が 198% CPU で数分回り続けた。規則側で
   **未到達のものだけを積む**ようにして、`queue.is_empty()` そのものを不動点の定義にした。

4. **空白識別子は「候補から外す」ではなく「根」**（honnef 9.9 "objects named the blank
   identifier are used"）。参照カウントの下では区別が要らなかったが、到達可能性では
   「候補でない」と「根である」は別物で、前者だと**そこから出る辺が切れる**:

   ```go
   var _ = initDebug()                    // restic internal/debug の 6 関数を生かしていた
   var _ unwrapper = wrappedRetryError{}  // rclone fserrors
   var _ credential = &otherKey{}         // nats certstore
   var _ = _SoftwareRequiresGOVERSION1_12 // prometheus tsdb/goversion/init.go
   ```

   これで corpus の新規偽陽性 9 件（restic 6 / rclone 1 / nats 1 / prometheus 1）が
   全部消えた。prometheus の 1 件は**定数と同じファイルを見ていても分からない**:
   定数は `goversion.go`、それを生かす `var _` は隣の `init.go` にある。
   「本当に誰も使っていないのか」は `go list -f '{{.GoFiles}}'` で
   *パッケージ全体*のファイルを出してから言うこと。
   honnef は const / type / var / `func _()` の 4 箇所すべてで `g.use(obj, by)` していて、
   パッケージレベルなら `by` は nil ＝ 根。4 箇所とも合わせた。

`iface_method_names` と `used_methods` は名前キーなので、到達可能性と**不動点で回す**
（到達したメソッドがさらに何かを生かしうる）。名前集合には
「このパッケージの候補でない使用済みオブジェクト」（＝インポート先・ローカル・フィールド）を
そのまま残してある —— 旧実装の緩さのうち、到達可能性が正当に狭める部分だけを狭めたかった。

**#79 — go1.26 の `new(expr)`**

同じ dapr のもう 1 件は `//nolint:gosec`:

```go
repeats = new(uint32(repetition))   // G115: integer overflow conversion int -> uint32
```

go1.26 で `new` は型だけでなく**値**を取るようになった。go/ssa は

```go
case "new":
    alloc := emitNew(fn, typeparams.MustDeref(typ), pos, "new")
    if !fn.info.Types[args[0]].IsType() {
        v := b.expr(fn, args[0])
        emitStore(fn, alloc, v, pos)
    }
```

と**引数を評価して格納する**。guff の `emit_new_builtin` は引数を丸ごと無視していたので、
`new(...)` の中に書かれた変換は SSA に現れず、`Convert` を読む G115（と、原理的には
`new(expr)` の中を見る全ての SSA チェック）から不可視だった。型検査は既に go1.26 形を
通していた（`builtin_new` が型として試してから値として試す）ので、抜けていたのは SSA だけ。

golden の gosec case は `go 1.24` だったので `go 1.26` に上げた。再生成した差分は
新しい 1 行だけ —— 言語バージョンの引き上げで他の 79 件は 1 つも動かなかった。

**辺はパッケージローカルの宣言だけ張る。** 最初は `info.uses` の全ての使用を辺にしていたが、
到達可能性が判定するのは**このパッケージのトップレベル宣言だけ**で、インポート先・ローカル
変数・フィールドは別の経路で決まる。全部入れるとグラフが一桁大きくなり、`regress --profile
full` が 2.040s の基準に対し **2.200s**（許容 +0.150s）で落ちた。ターゲットが
`candidates ∪ roots` のときだけ辺を張り、辺集合を `HashSet` にして重複も潰すと
**1.980–2.020s** で 4 回連続 PASS ＝ 基準より速い。golden / isolate は byte 単位で不変。

なお `attributed` は**ターゲットが何であれ**立てる。「歩き切れなかった Ident は根」の
フォールバックは*歩いたかどうか*で決まるべきで、*指した先*で決まってはいけない。

**まだ模していない根が 2 つ**（記録のみ）: `//go:linkname` の対象と
`//go:cgo_export_*` を持つ関数。どちらも honnef は根にする。guff は
「歩き切れなかった Ident は根」というフォールバックを持つが、linkname は
Ident の使用ではなく**パッケージスコープの名前引き**なので、そこには掛からない。
corpus 内の `go:linkname` は containerd の vendored `x/sys` だけ。

**ゲート**: `unused` は `dead_cycle.go`（死んだ環＋生きた鎖を同じファイルに置く）を
Rust の単体テストと golden の両方に。`gosec` は `g115.go` に `new(型)` / `new(変換)` /
`new(拡大変換)` の 3 行。どちらも byte-exact 側で上流と突き合わせている。

### 2026-08-21（続き 8）— precision から recall へ: gosec G118 / G123 を移植した

前セッションで corpus 全体の guff-only は 6 件まで落ち、**残る差は「上流だけが出す」側**
——つまり未実装ルール——になっていた。dapr の golangci-only 6 件はその全部が gosec で、
内訳は **G118 が 5 件、G123 が 1 件**。両方入れて dapr は 1364/1364 の完全一致になった。

**G118（context propagation）** は 1 つの analyzer id に**無関係な 3 つの検査**が入っていて、
severity / confidence もそれぞれ違う。`issue_scores` が G402 に続いて 2 例目の
「メッセージを見て採点する」ルールになった。

| 検査 | 採点 | corpus での出現 |
|---|---|---|
| cancel が呼ばれていない | Medium/High | dapr ×4 |
| `go` の先が `context.Background`/`TODO` | High/Medium | dapr ×1 |
| 出口の無いループに `ctx.Done()` が無い | High/Low | 0 |

移植の分量はほぼ全部が **「cancel は呼ばれていないが、それでいい」形の列挙**である
（返す／クロージャが捕捉する／構造体フィールドに置いて別メソッドが呼ぶ／
パッケージ変数に置く）。ルール本体は 3 行しかない。

上流の挙動で**バグに見えるが仕様として写した**ものが 3 つある。どれも実際に findings を決める:

1. **ジェネリック型は `types.Identical` を通らない。** `func New[T any]() *Conn[T]` の
   複合リテラルの型は*インスタンス* `Conn[T]`、`(*Conn[T]).Close` のレシーバは
   **型引数を持たない origin**。`Identical` は型引数の本数が違う時点で false を返すので、
   `Close` が `c.cancel()` を呼んでいてもフィールド走査は届かない。
   dapr の `pluggable.GRPCConnector[TClient]` がまさにこれで報告されている。
2. **map に入れた cancel は追わない。** 走査の命令集合に `MapUpdate` が無い。
   dapr の `subscriber.retrySubscription`（`s.retryCancel[name] = cancel`）が該当。
3. **`c, _ := context.WithCancel(ctx)` は報告される。** go/ssa は blank lvalue に捨てる前に
   `Extract #1` を発行するので、参照 0 の cancel として見える。
   fixture で一番「偽陽性に見える」行なので、そう書いて置いてある。

「出口の無いループ」検査は Tarjan の SCC で、**SCC の外に出る辺が 1 本でもあれば対象外**。
`for { … return … }` は return ブロックへの辺を持つので落ちる ＝ 本当に終わらないループだけが残る。
corpus 14 リポで 0 件なのはそのため。再帰の Tarjan は**フレームを明示したループ**に置き換えたが、
後続の訪問順（＝ SCC 内のブロック順＝報告位置）が変わらないように書いてある。

**土台の欠落を 1 つ見つけた。** go/ssa の `CreatePackage` は syntax の無いパッケージに対し、
export data から**メソッドも Member として作る**（`named.Method(i)` を回している）。
guff は import の member を遅延生成し、しかも**パッケージレベルのオブジェクトだけ**なので、
`object_method` 経由で作られるメソッドは `pkg: None` の合成シェルになる。
その結果 `(*http.Request).Context` が `net/http` のものだと分からず、
**`http.Handler` の中の `go` が丸ごと黙っていた**。宣言パッケージは
型検査オブジェクト側に残っているので、`f.pkg` が無いときは `f.object.pkg()` を見るようにした
（`gosec_g118::func_pkg_path`）。SSA 側を直すのが本筋だが、それは
`buildir` を含む全 analyzer の member 集合を動かすので、ここでは踏み込んでいない。

**G123（TLS 再開が `VerifyPeerCertificate` を迂回する）** は短い。
`tls.Config` の 5 フィールドへの `Store` を数える**在庫表**であって dataflow ではない:
`VerifyPeerCertificate` が入っていて、`VerifyConnection` も
`SessionTicketsDisabled: true` も無ければ報告。再開されたセッションは証明書チェーンを
提示しないので、そのコールバックは走らない。

在庫表なので、**config が別の関数で組まれると追えない**。
`GetConfigForClient: func(…) { return direct(), nil }` は上流も報告しない
（`direct()` の Store は別の関数の値をキーにしている）。
クロージャの中で組めば両方報告される。fixture はその 2 つを並べて置いた。

**ゲート**: `gosec` の golden case に fixture を 2 本足した（`g118.go` / `g123.go`）。
どちらも 1 行ごとに `// FINDING` / `// silent` を書いてあり、その印は
golangci-lint 2.12.2 の実行結果と byte 単位で突き合わせている。
Rust の単体テストは同じ fixture をスタブ宇宙で通す
（`context` / `crypto/x509` のスタブと、`*http.Request` の `Context()` を足した）。

**dapr**: guff=1364 golangci=1364 both=1364 P=100.0% R=100.0%。

**次**: gosec の未実装は G113 / G116–G117 / G119–G121 / G201 / G304–G305 / G307 と
G7xx の完全 taint エンジン。corpus の他リポで残る golangci-only は thanos の 3 件
（unparam ×2 / staticcheck の malformed json tag ×1）だけになった。

### 2026-08-21（続き 9）— SA5008 は「タグを読む」ところから間違っていた（穴 5 つ）、そして残る 2 件は unparam ではない

続き 8 で recall gap は corpus 全体で 3 件になった。その 3 件を読んだら、**2 件は unparam の
バグですらなかった**。

#### 残り 2 件の正体: `ctrlflow` の noreturn が無い

```
thanos examples/interactive/interactive_test.go:49  unparam  exec - cmd always receives "sh"
thanos test/e2e/compatibility_test.go:56            unparam  testPromQLCompliance - queryFrontend is unused
```

`queryFrontend` は 124 行目と 141 行目で**使われている**。それでも上流が "is unused" と言うのは、
`buildssa` が `prog.SetNoReturn(cfgs.NoReturn)` を呼び、go/ssa の `emitCall` が
**noreturn な静的呼び先の直後に `Panic` を置いて `unreachable.noreturn` ブロックに切り替える**
から（x/tools v0.44.0 `go/ssa/emit.go:512-531`）。この関数は 1 行目が `t.Skip(...)` なので、
**後ろが丸ごと消える**。guff には ctrlflow が無いのでブロックが生き残り、参照も残る。

最小再現（両方 golangci-lint 2.12.2 は報告、guff は沈黙）:

```go
func helper(t *testing.T, flag bool) {
    t.Skip("interactive")
    if flag { t.Log("yes") }      // => helper - flag is unused
}
```

`exec` のほうも同じ根。`interactive_test.go` のテスト関数は 1 つだけで、その 1 行目が
`t.Skip(...)`。`exec("cp", …)` は全部その中なので、生きているのは `createData` の
`exec("sh", …)` 4 本だけ ＝「cmd always receives "sh"」。こちらも最小再現を取った。

**移植規模**: `go/cfg`（約 1100 行）＋ `ctrlflow`（278 行、fact ベースの手続き間解析）
＋ go/ssa 側 12 行。**土台の変更で staticcheck / govet / unparam の findings が同時に動く**ので、
単独セッション＋フルゲートでやること。`sa5011.rs` と `lostcancel.rs` は既にこの fact を
**名前ベースで代用**していて、どちらも DEFERRED コメントで ctrlflow を名指ししている。
移植すればそこも畳める。**まだやっていない。**

#### 3 件目を直したら、SA5008 に穴が 5 つあった

残る `malformed json tag` は素直な未移植だったが、上流
（`honnef.co/go/tools@v0.7.0/staticcheck/sa5008/`）を guff と並べて読むと**5 つ**出てきた。
そのうち **2 つは guff-only の偽陽性**で、どのゲートにも出ていなかった。

| # | 種類 | 内容 |
|---|---|---|
| 1 | **精度** | `parse_struct_tag` が構造の壊れたタグで `Err` を返していた。上流は `break` して黙る |
| 2 | **精度** | go-flags を import しているとき `choice` / `optional-value` / `default` の重複は免除される |
| 3 | 再現性 | 手書きの `unquote` が `"` と `\` しか見ていなかった |
| 4 | **再現性** | `validateJSONTag`（`jsonv2.go` 288 行）が丸ごと未実装 |
| 5 | 一致 | 同じキーが複数値のとき上流は `v[0]` だけ検証する |

**#1 が一番効く。** 上流の `parseStructTag` は `reflect.StructTag` のスキャナのコピーで、
`name:"value"` に見えないものが来たら**走査を止めるだけ**。エラーになるのは
`strconv.Unquote` が失敗したときだけ。guff は前者もエラーにしていたので、

```go
`notatag`            `json:"b" trailing`            `json`            `json:"e`
```

の 4 形すべてに `unparseable struct tag: malformed struct tag` を出していた。上流は全部沈黙。
**普通のコードに出る偽陽性**である（`json:"b" trailing` のような形は珍しくない）。

**#3 はタダで直った。** `crates/guff-staticcheck/src/gostd/strconv.rs` に
Go の `strconv` の移植（SA100x のオラクルでゲート済み）が既にあったので、手書きを捨てて
`strconv::unquote` を呼ぶだけ。エラー文言が `invalid syntax` で上流と一致するのもこれのおかげ。

**#2 は corpus では絶対に捕まらない。** 14 リポに go-flags を使うものが無い。
上流のソースを guff と並べて読んで初めて出た。**「corpus を増やすのが効く」の実例。**

#### 意図的に残したもの

- `fakexml.StructFieldInfo` の `invalid XML tag: %s`（`fakexml` ＋ `fakereflect` で約 920 行）。
  DEFERRED、モジュールヘッダに記録。
- `invalid UTF-8 in JSON object name` は **Rust では到達不能**。Go の `strconv.Unquote` は
  不正なバイト（`ÿ`）を含む文字列を返せるが、Rust の `String` は持てず、`gostd` の移植は
  対応する `char` にデコードする。分岐は上流と形を揃えるために残してある。

**ゲート**: `sa5008/bad.go` と `ok.go` を書き足して、golden の `staticcheck-sa` が
golangci-lint 2.12.2 と byte 単位で突き合わせる（SA5008 が 1 件 → 16 件）。
go-flags の免除だけは外部モジュールが要るので Rust の単体テスト＋スタブで。
なお fixture の doc コメントは**型名で始めてある** —— この case は ST も走るので、
ST1021 が SA5008 の findings を埋めてしまうため。

**thanos**: guff=432 golangci=434 P=100.0%（golangci-only は上記 unparam 2 件だけ）。

### 2026-08-21（続き 10）— corpus を 1 本足したら wsl_v5 の偽陽性が 176 件出た

続き 9 で「negative space（撃たないこと）を保証しているものが無い」と書いた。
その直後に **corpus に authelia を 1 本足した**ら、いきなり
**guff-only 193 件**（wsl_v5 176 / nolintlint 15 / gosec 2）が出た。
golangci はこのリポで **0 件**である。既存の 14 リポでも golden でも isolate でも、
どのゲートにも一度も出ていなかった。

wsl_v5 の 176 件は **3 つのバグ**に分かれた。3 つとも「上流は黙るのに guff が撃つ」形で、
3 つとも上流のソースを guff と並べて読んで初めて分かった。

| # | 上流の挙動 | guff | 件数 |
|---|---|---|---:|
| 1 | 既定の check 集合（`assign-expr` は **off**）では、**識別子を共有していれば** assign が expr 文に cuddle してよい | 無条件に「invalid statement above assign」 | ~30 |
| 2 | `checkExprStmt` は `checkCuddling(..., enforceLimit=false)` を渡すので、**expr に cuddle 上限を課さない** | `cuddle-max-statements` を課していた | ~109 |
| 3 | `{` と最初の文の間のコメントは**内容**であって空行ではない | コメント行を空行と見て leading-whitespace | ~35 |

#1 の根拠は wsl v5.8.0 `wsl.go` の

```go
if _, ok := w.config.Checks[CheckAssignExpr]; !ok {
    if _, ok := previousNode.(*ast.ExprStmt); ok && w.hasIntersection(stmt, previousNode) {
        prevIsValidType = prevIsValidType || ok
    }
}
```

`hasIntersection` は両文の識別子集合の交差で、**型名・universe 定数・`nil`・パッケージ名・`_` を除く**
（`identsFromNode` / `isTypeOrPredeclConst`）。この除外は型情報が要るので、guff 側は
`Info.Uses` / `Info.Defs` から「落とす ident の node id 集合」を 1 パッケージにつき 1 回作って渡している
（`ident_skip_set`）。除外を省くと交差しやすくなり、**今度は撃つべき所で黙る**ので手は抜けない。

#3 で土台の制約を 1 つ踏んだ: **本番の typecheck は `PARSE_COMMENTS` なしでパースするので
`File::comments` は空**。`funlen` が同じ問題を「必要なときだけ `COMMENTS_ONLY` で再パース」で
解いていたので同じ手を使ったが、**再パースは自前の `FileSet` を持つ**ため、
コメントの `Pos` を文の `Pos` と比較できない。同じソースを 2 回パースしても**行番号は一致する**ので、
比較を行番号に寄せてある。

**結果**: authelia の guff-only は 193 → 19。残りは `//nolint:gosec` のカスケード 15
（上流の gosec が撃つ所で guff の DEFERRED ルールが撃たない）と gosec の偽陽性 2。

**ゲート**: `compat/isolate/fixtures/wsl_v5/bad.go` に 3 形を追記した。
どれも**上流が黙る**形なので、fixture としては「findings が増えないこと」を見ている
（guff=3 golangci=3 で一致）。これが崩れたら isolate が落ちる。

**教訓**: golden も isolate も 99/99・116/116 で緑のまま、この 176 件は存在していた。
fixture は「撃つ」形しか書かれていないので、**偽陽性はゲートでは見つからない**。
見つかったのは corpus に 1 本足したからである。続き 9 の
`corpus/shapes.py`（言語の形）とは別の軸——**実在するコードの書き癖**——が効いた。

### 2026-08-21（続き 11）— `//lint:file-ignore` はファイルで止まらない

続き 10 と同じ形の続きで、**hunt を 15 リポ全部回した**ら nats-server に guff-only が
3 件出た。うち 1 件は `unused` で、**どのゲートにも一度も出ていなかった**新しい形である。

```
server/raft_helpers_test.go:232  unused  func (*cluster).addRaftNode is unused
```

`addRaftNode` は本当にどこからも呼ばれていない（リポ全体を grep して宣言 1 件のみ）。
それでも上流が黙るのは、**`type cluster` を宣言している別のファイル**
`jetstream_helpers_test.go` の先頭に

```go
//lint:file-ignore U1000 Avoid detecting as unused code
```

があるからで、上流の該当箇所は「ファイル内のオブジェクトを used にする」で終わっていない:

```go
if obj, ok := obj.(*types.TypeName); ok {
    if typ, ok := types.Unalias(obj.Type()).(*types.Named); ok {
        for method := range typ.Methods() { g.use(method, nil) }   // ← ファイルを跨ぐ
    }
    if typ, ok := obj.Type().Underlying().(*types.Struct); ok {
        for field := range typ.Fields() { g.use(field, nil) }
    }
}
```

`types.Named.Methods()` は**そのファイルに書かれたメソッドではなく、その名前付き型の
メソッド全部**を返す。nats-server はメソッドを兄弟の `*_test.go` に散らしているので、
「メソッド自身の位置」で濾していた guff は素通りさせられなかった。

#### 濾すのと根に置くのは別のこと

guff は `collect_lint_ignores` の結果を**報告直前のフィルタ**として使っていた。
上流は `g.use(obj, nil)` ＝ **到達可能性グラフの根**に置く。差は 2 つ出る:

| | 濾すだけ | 根に置く |
|---|---|---|
| 無視された型のメソッド（別ファイル） | 報告される | used |
| 無視された宣言が**参照しているもの** | 到達不能のままで報告される | used |

2 行目は fixture の `keptAlive` が押さえている。無視された関数からしか呼ばれない関数は、
上流では生きていて guff では死んでいた。**同じ 1 行の修正で両方消える**ので、
フィルタをやめて根に積む形に直した。

**ゲート**: `crates/guff-unused/tests/testdata/fileignore/` に **2 ファイル 1 パッケージ**の
fixture を置いた。directive のあるファイルに型、無いファイルにメソッド —— nats-server の形
そのままである。golden の `cases/unused` に `sources.txt` 経由で載せてあり、
golangci-lint 2.12.2 が撃つ 2 件（`(*plain).unusedMethod` と `unusedFree`）と
byte 単位で一致する。**撃つ側と黙る側を同じファイルに並べてある**のが要で、
「全部黙らせる」修正では通らない。修正前のバイナリでこの fixture を回すと
guff だけが 4 件（`inPlainFile` と `keptAlive` が余分）出る。

Rust 側の単体テストも同じ fixture を使う。1 ファイル 1 パッケージ前提だった
`support::typecheck_pkg` に複数ファイル版を足してある —— **ファイルを跨ぐ規則は
1 ファイルのハーネスでは書けない**ため。

**nats-server**: `unused` は 0 件で golangci-lint と一致（`./server/...` を単独 config で実測）。
残る guff-only は ineffassign ×1 と SA5011 ×1 で、どちらも続き 7 に記録済みのもの。

#### hunt 15 リポの現在地（2026-08-21）

| target | 残り |
|---|---|
| go-redis / restic / rclone / cli / traefik / coredns / prometheus / dapr / argo-cd / atlas | **差分なし** |
| nats-server | ineffassign ×1 / SA5011 ×1（この回で unused ×1 を解消） |
| jaeger | revive ×2 —— **§6 の「revive の importer 盲目には追従しない」そのもの**（`time-naming` / `epoch-naming`）。恒久差分 |
| thanos | unparam ×2 —— ctrlflow の noreturn 未移植（続き 9） |
| gitea | nolintlint ×1 —— 上流 SA4006 の偽陽性のカスケード（続き 7） |
| authelia | wsl_v5 ×2 / gosec ×1 / nolintlint ×15 |

**次にやること**

1. authelia の wsl_v5 ×2。上流 `checkError` の 2 つの分岐が guff に無い ——
   (a) `previousIdents` は `*ast.AssignStmt` と `*ast.DeclStmt` からしか作らない
   （`if uri, err = …; err == nil {}` は交差を作らない）、
   (b) 代入と `if` の間のコメントが**別の行**にあれば return。
2. authelia の gosec G703 ×1（`cmd_adr.go:131`）。上流の taint 設定は
   `os.ReadFile` を source にも sink にもしているので、どちらの経路で汚れているかを
   最小再現で切り分ける。
3. `ctrlflow` / `go/cfg` の移植（続き 9）。thanos の 2 件と、`sa5011.rs` /
   `lostcancel.rs` の名前ベース代用が同時に畳める。

---

### 2026-08-21（続き 12）— authelia の残り 3 件: wsl_v5 の 2 つの早期 return と、死なない taint

続き 11 の「次にやること」1・2。authelia の guff-only 18 件のうち guff 自身の欠陥は 3 件で、
残り 15 件は「guff が出さない finding」に nolintlint がカスケードしたものだった。

#### wsl_v5 —— `checkError` は 3 か所ずれていた

上流 `checkError`（wsl v5.8.0 `wsl.go:752`）は「err 代入と `if err != nil` の間の空行」を消す
指示を出す。guff に無かった早期 return が 2 つ、報告位置が 1 つ違っていた。

| # | 上流 | guff |
|---|---|---|
| 1 | `previousIdents` は `*ast.AssignStmt` の LHS と `*ast.DeclStmt` の名前**だけ**から作る | `find_lhs` が `if` の cond まで見ていた |
| 2 | 代入と `if` の間のコメントが**自分の行**にあれば return（trailing のときだけ空行を消す） | コメントを見ていなかった |
| 3 | 報告位置は削除範囲の先頭 `file.LineStart(previousEndLine + 1)` ＝ **最初の空行の 1 桁目** | 上の代入文の先頭 |

#1 は authelia の `parseAttributeURI` の

```go
if uri, err = url.ParseRequestURI(value); err == nil { … }

if err != nil {
```

で、`if` の init が err を代入していても上流の交差は空になる。

**#3 は fixture に「撃つ側」を足して初めて出た。** それまでの `wsl_v5` fixture には
`err` の finding が 1 件も無く、この check は**報告に到達しない 2 形だけ**でゲートされていた。

#### gosec G703 —— SSA では 2 つの値、名前では 1 つ

上流の taint は SSA 上で動く。`raw, _ = os.ReadFile(p)`（宣言された source）と
`raw, _ = json.Marshal(cfg)`（source ではない）は**別の値**なので、後者は前者の taint を
運ばない。guff は「この名前に source を代入したことがあるか」の平坦な集合を持っていて、
それでは表現できない。authelia の `cmd/authelia-gen/cmd_adr.go` がまさにその形である
（config を読む → unmarshal → カウンタを増やす → marshal → 書き戻す）。

代入と sink を**位置順に再生**すれば、SSA を持たなくても直線コードでは同じ答えになる:
後の代入が汚れていなければ taint は**死ぬ**。同じ再生で真陽性が 1 件増えた ——
代入を通じた伝播（`content := strings.ReplaceAll(string(data), …)`）は
収集専用パスには無かったので、`internal/suites/utils.go:270` の `//nolint:gosec` が
書かれた理由である G703 を guff も出すようになり、カスケードしていた nolintlint も消えた。

#### 「呼び出しを通す」は 3 種類ある（nightly が教えてくれた）

最初の実装は**代入の右辺にある呼び出しを無条件に通した**。PR の `oss-nightly` が
grafana で落ちて分かったのは、上流がここを 3 つに分けていること
（`taint/taint.go:583-630`）:

| 呼び先 | 引数の taint は戻り値へ |
|---|---|
| 外部（本体が無い＝ stdlib など） | **流れる** |
| 内部（本体がある） | `doTaintedArgsFlowToReturn` が認めたときだけ |
| static callee が無い（関数型の変数） | **流れない** |

grafana の `summary_test.go` は 3 行目そのもので、

```go
body, err := os.ReadFile(path)             // source
summary, _, err := reader(ctx, uid, body)  // reader はローカル変数
out, err := json.MarshalIndent(summary, …)
os.WriteFile(gpath, out, 0600)             // 上流は黙る
```

無条件に通すと 2 ホップ先の sink で撃ってしまう。guff は手続き間解析を持たないので
**1 行目だけを採用し、残り 2 つは通さない**（型変換 `string(b)` / `[]byte(s)` は
上流の `*ssa.Convert` に当たるので通す）。**sink 側の述語は従来どおり**で、
そちらは呼び出しの中を見てよい（`os.Stat(f(os.Getenv(…)))`）。

**教訓**: ローカルの `--oss --tier pr,nightly` を**その PR のブランチで**回すこと。
この回は syncthing のブランチ（main 由来）で回していたので、G703 の変更が
入っていないバイナリを測っていた。

**ゲート**:

- `compat/golden/cases/wsl-v5` を新設。wsl_v5 は isolate しか持っておらず、
  そのキーには **column が無い**。golden は `path:line:col:linter:severity:text` を
  正規化なしで見る。fixture は `compat/isolate/fixtures/wsl_v5/bad.go` を共有していて、
  そこに**黙る形と撃つ形を並べて**ある。
- gosec の golden に `bad/bad.go:160`（taint が生き残る）を足し、ok.go の再代入形
  （taint が死ぬ）と対にした。**片方だけでは「全部黙らせる」修正が通る。**

**authelia**: guff-only 18 → 14（全部 nolintlint のカスケード）。

---

### 2026-08-21（続き 13）— corpus に syncthing を足したら、modernize の 2 つが接頭辞なしで出荷されていた

続き 10 の方法（**corpus を 1 本足す**）をもう一度回した。hunt の 15 リポは
残差が 1 桁台まで落ちていたので、新しい形は新しいリポからしか出てこない。

#### 候補の選び方と、比較が成立しなかった 1 本

`.golangci.yml` が v2 で、かつ**有効な linter が多い**ものを探した。
`syncthing` は `linters.default: all` から 42 を disable ＝ **約 75 linter**が実コードに当たる。
promlinter / sloglint / godoclint / gosmopolitan / forcetypeassert / errchkjson / fatcontext は、
**どの corpus リポでも実コードに当たったことが無い**。
（otel-collector と bubbletea も試したが、root モジュールが小さく 0 vs 0 だった。）

syncthing は**素のチェックアウトではビルドできない**。`lib/api/auto` の `Assets()` が
生成ファイルにあるためで、**golangci-lint はコンパイルエラーがあると typecheck の 1 件だけを出して
リポ中の finding を全部落とす**。初回はそれで「guff が 547 件でっち上げた」ように見えた。
リポは `noassets` build tag をこのために持っているので、`hunt.json` に
`build_tags` を持たせて **両方のツールに渡す**ようにした（`compat/hunt.sh`）。
**生成器ではなくタグを運ぶ**ほうが安く、再現性も高い。

#### 出てきたもの

golangci-lint は modernize の finding を **`<modernizer>: <message>`** で描画する
（Diagnostic の Category）。guff の 25 checker のうち **`any` と `plusbuild` の 2 つが
Category を設定していなかった**ので、メッセージを素で出していた。syncthing だけで **118 件**、
しかもテキストが違うので**差分の両側に同時に並ぶ**。`normalize.py` はこの接頭辞を
剥がしていたが 2026-08-13 に測って外してあり、以来 8 日間これを捕まえるものが無かった。

同じ fixture を初めて突き合わせて、さらに 4 つ:

| check | guff | 上流 |
|---|---|---|
| `stringsseq` | `Ranging over strings.Split allocates a slice; consider using strings.SplitSeq` | `Ranging over SplitSeq is more efficient` |
| `stringsseq` | 直接形のみ | `lines := strings.Split(…)` の下の range も（その変数の**唯一の使用**であるとき） |
| `slicescontains` | `loop can be modernized using slices.Contains` | `Loop can be simplified using slices.Contains` |
| `slicescontains` | `found := false` 形は代入文を報告 | どの形でも range 文を報告 |
| `fmtappendf` | `[]byte(fmt.Sprintf) can be modernized using fmt.Appendf` | `Replace []byte(fmt.Sprintf...) with fmt.Appendf` |
| `minmax` | `<` 演算子の位置 | 比較**式**の位置（`compare.Pos()`）—— 2 桁左 |

#### あるべきだったゲート

`crates/guff-style/tests/testdata/modernize/` の 28 fixture のうち golden を持っていたのは
**6 つだけ**だった。残り 22 は「どれかのメッセージがこの部分文字列を含む」という
Rust のアサーションで守られていて、それは
**「golangci-lint がこの行をこう出力する」とは別の主張**である。
`compat/golden/cases/modernize` を新設して 19 fixture を載せた。上の 6 バグは全部その中にある。

意図的に外した 2 つ:

- `plusbuild.go` は `//go:build linux`。他のマシンでは golangci-lint が何もコンパイルせず、
  **golden が空ファイルの記録になる**（21 本目の教訓）。ケース側に環境非依存のコピーを置いた。
- `newexpr.go` は **golangci-lint 2.12.2 自身がクラッシュする**
  （`goanalysis_metalinter: newexpr: index out of range [0] with length 0`）。
  上流の答えが存在しないので記録できない。`run.sh` は再現できなかった golden を書かない。

**syncthing**: guff-only 141 → 20、golangci-only 188 → 66、P=96.5% R=89.4%。

**次にやること**（syncthing が名指ししたもの）

1. **`lib/model` が ill-typed**（`GUFF_DEBUG_ILL_TYPED=1` で 3 件）。
   `*indexHandler does not satisfy suture.Service (wrong type for method Serve)` が 2 件
   —— `have` が**空**なのでメソッドのシグネチャを解決できていない —— と
   `model.go:3482: invalid append: argument S is not a slice`（型パラメータ）。
   **この 1 パッケージで forcetypeassert 11 / godoclint 3 / sloglint 2 / fatcontext 1 /
   unparam 1 が丸ごと落ちている**。hunt tier には health ゲートが無いので誰も落ちない。
2. **`forcetypeassert` の column** —— 上流は `n.Pos()`（代入文の先頭）、guff は `tok_pos`（`:=`）。
   行は一致するので isolate / OSS のキーでは見えない。golden ケースが要る。
3. **SA4016 の偽陽性**（`lib/fs/filesystem.go:182`）。上流は `x | y` の `y` が
   **`= iota` と書かれた同一パッケージの定数**か **整数リテラル 0** のときだけ撃つ
   （`sa4016.go:55-100`）。`OptReadOnly = os.O_RDONLY` は値 0 だが spec の値が
   SelectorExpr なので上流は黙る。
4. gosec の未実装 taint 21 件（G702 / G703 / G706 / G710）と nolintlint のカスケード 6 件。
5. **`uniq-by-line` と linter 順序**。syncthing のように 1 行に複数 linter が撃つリポでは、
   どちらの finding が残るかが順序で決まる。guff は `from_linter` 名でソートしてから
   落としているが、上流は metalinter の内部順序を経由する。再走で差分が動いた形跡があるので、
   hunt の config で `uniq-by-line: false` にして**比較を順序非依存にする**のが先。

---

### 2026-08-22（続き 14）— minmax は上流の 2 パターンのうち 1 つしか無く、「等しい」の定義も違った

続き 13 の残り。syncthing が `minmax: if statement can be modernized using min/max` を
**9 件**出していて guff は 0 件だった。**文言が手掛かり**である ——
guff のメッセージは "if/else statement"、上流の 2 つ目のパターンは "if statement"。

#### パターン 2

```go
v := x
if v > y {
    v = y
}
```

`lhs0 = rhs0` が `if a < b { lhs = rhs }` の**直前**にあり、else が無い形
（x/tools `modernize/minmax.go:139-207`）。`if` の**上**の文が要るので
`IfStmt` ノードではなくブロックを走る。上流が明示的に弾く
`select` の comm clause（`case v := <-ch:`）は、その代入がブロックの文リストではなく
clause の `Comm` なので**自動的に外れる**。

照合では `lhs0` が `rhs0` の代わりを務めてよいが、**fix は `v = min(v, y)` と書いてはいけない**
—— `=` が `:=` だったかもしれないため。

#### `astutil.EqualSyntax` は「同じ値」ではない

パターン 2 だけでは 9 件のうち 4 件しか戻らなかった。残り 5 件は「等しい」の定義で詰まる:

```go
count := len(a)
if len(b) < len(a) {
    count = len(b)
}
```

上流はオペランドを `astutil.EqualSyntax` で照合する ——
**書かれた形**、識別子は**名前**で比較 —— なので `len(a)` は `len(a)` と一致する。
guff は `code::same_non_dynamic` を使っていた。あれは「2 つの式が**同じ値**を表すか」を問うので
呼び出しを問答無用で拒み、しかも 4 種類のノードしか見ないので
`len(buf)-written` と `len(buf) - written` すら一致しなかった。

`code::equal_syntax` がその移植である。**両方のパターンが今はこれを使う**（上流と同じ）。

**ゲート**: `crates/guff-style/tests/testdata/modernize/minmax.go` を新設し、
`modernize` の golden ケースに載せた。**撃つ 5 形と黙る 3 形**（上が別の変数 /
オペランドが代入と無関係 / float（`maybeNaN`））、それにパターン 1 の対照を並べてある。
6 キーすべてが golangci-lint 2.12.2 と byte 単位で一致。

**syncthing**: modernize の golangci-only 11 → 4、P=96.6% R=90.5%。

---

### 2026-08-22（続き 15）— `fired` は「検証済み」ではない、二度目: forcetypeassert の fixture は 6 行だった

続き 13 の「次にやること」2。`docs/COVERAGE.md` は forcetypeassert を `fired` と数えている。
それを撃たせていたのはこれである:

```go
func bad() {
    var a any
    _ = a.(int)
}
```

finding 1 件と、対になる 3 行の `ok.go`。**Phase 3 が単一 check の linter について
名指ししている状況そのもの** —— `fired` は「check が起きた」であって
「上流と一致する」ではない（§3 の goheader と同じ）。

一致していなかった。上流は**どこでも `n.Pos()`** を報告する ——
`*ast.AssignStmt` の最初の左辺、`*ast.ValueSpec` の最初の名前、
裸の `*ast.TypeAssertExpr` のオペランド。guff は `:=` トークンと assertion の `(` を
報告していた。**毎回同じ行**なので、isolate のキー（`path:line:linter:message`）でも
OSS のキーでも見えない。syncthing はこの形を 44 回書く。

fixture は 8 件に置き換えた —— 報告に至る経路を全部通る（blank 代入 /
index 式を経由する `:=` と `=` / `var` spec / 条件中の裸の assertion / 引数中の裸の assertion /
"right hand must be only type assertion" の 2 通り（呼び出しに埋もれる・右辺が 2 値））——
そして黙るべき 4 つを並べた（comma-ok の 2 綴り、`any` への assertion（上流の `isAny`）、
そして `TypeAssertExpr` が `Type` を持たない type switch）。

`compat/golden/cases/forcetypeassert` が column ごと固定する。

---

### 2026-08-22（続き 16）— ill-typed は差分に出ない、三度目: syncthing の `lib/model`

続き 13 の「次にやること」1。`GUFF_DEBUG_ILL_TYPED=1` で syncthing を回すと
`lib/model` に **`go build` が受理する 3 件のエラー**が出ていた。
**ill-typed は差分ではない** —— ill-typed パッケージで走らない analyzer はただ黙るだけなので、
hunt では「golangci-only」としてしか見えない: forcetypeassert ×11 / godoclint ×3 /
sloglint ×2 / fatcontext ×1 / unparam ×1、**全部この 1 パッケージから**。

原因は 2 つ。

#### 1. `append` が欲しいのは core type であって underlying ではない

```go
func without[E comparable, S ~[]E](s S, e E) S {
    …
    return append(s[:i], s[i+1:]...)
}
```

型パラメータの *underlying* は制約インターフェースであり、
「全メンバーがスライスか」を答えるのは**型集合**のほうである。go/types はここで
`coreType(S)` を訊く。guff は `under(S)` を訊いて
「argument S is not a slice」と答えていた。`clear` / `delete` / `close` にも
同じ `DEFERRED: underIs` が付いていた。**`common_under` は既にあった**
（`unsafe.Slice` / `unsafe.String` が使っている）ので、4 つともそれを使う。

#### 2. 制約の検証はメソッドを待たなければならない

```go
type box[S Service] struct{ v S }
type handler struct{}
func (h *handler) Serve(ctx context.Context) error { … }
type registry struct { h *box[*handler] }   // ← ここで検査される
```

インターフェース制約の充足には型引数の**メソッド**が要る。そしてメソッドのシグネチャは
**そのメソッド自身のオブジェクト宣言**で解決される —— インスタンス化が
*別の型の宣言の中*に書かれている時点では、まだ走っていない。
guff はここをインラインで検証していたので `*handler` の `Serve` を型が付く前に読み、
`wrong type for method Serve; have <空>` と報告していた。**`have` が空なのが目印**である。

同じ形でも**値の位置**（`func New() *box[*handler]`）なら常に通っていた ——
関数本体は最後に検査されるからで、これが「たまに出る」ように見えていた理由。

Go はこれを遅延する（`typexpr.go` の `check.later(func() { … verify … })`）。
guff のコメント自身が「verify だけが遅延を要する部分だが、インラインでやっている」と
書いてあった。**遅延した。** `mono.record_instance` も一緒に動かし、
AST の借用より長生きするので位置を受け取る変種を足した。

**結果**: syncthing の `lib/model` は ill-typed 3 → **0**。
hunt の golangci-only は forcetypeassert 11 → **0**、godoclint 4 → 1、
sloglint 2 → 0、fatcontext 1 → 0。**R = 90.5% → 93.2%**。

**ゲート**: `crates/guff-types/tests/check_files.rs` に 4 本。
struct フィールドのインスタンス化が通ること、**本物の制約違反はいまも報告されること**、
4 つの builtin が制約付き型パラメータを受けること、そして
**型集合がスライスだけでない型パラメータは `append` がいまも拒むこと**。

**教訓**: `compat/health.py` の ill-typed ゲートは OSS tier にしか掛かっていない。
hunt tier に足したリポは、パッケージが丸ごと落ちていても誰も落ちない。

---

### 2026-08-22（続き 17）— SA4016 は上流の 2 分岐のうち 1 つしか無く、`^` が `?` と描画されていた

続き 13 の「次にやること」3。syncthing の `lib/fs` に

```go
const (
    OptAppend    = os.O_APPEND
    OptCreate    = os.O_CREATE
    OptReadOnly  = os.O_RDONLY   // 0
    …
)
flags := OptAppend | OptCreate | OptExclusive | OptReadOnly | …
```

があり、guff は「always equals OptAppend | OptCreate | OptExclusive」と撃っていた。
上流は黙る。理由は**分岐が 2 つある**こと（`staticcheck/sa4016/sa4016.go:55-100`）:

1. 右オペランドが**このパッケージの**定数を指す `*ast.Ident` で、値が 0 で、
   **かつその spec が文字どおり `name = iota` と書かれている**とき ——
   `1 << iota` の書き間違いだろうと読んで、メッセージにそう書く;
2. 右オペランドが整数リテラルのとき —— `pattern.IntegerLiteral` は
   「整数の basic literal と単項 `+` `-` **だけ**」（`pattern/pattern.go:318`）。

guff は 2 だけを持ち、しかも `code::is_integer_literal` を訊いていた。
あれは**任意の式の定数値**を評価するので、名前付き定数も「0 である」と答えてしまう。
他に 11 箇所の呼び手があり、そちらでは正しい問いなので**共有ヘルパは触らず**、
この check 側で「形」も訊き、欠けていた ident 分岐を足した。

#### `^` が `?` になっていた

`render.rs` の演算子表に `XOR` / `SHL` / `SHR` / `AndNot` が無く、
`_ => "?"` が黙ってそれを飲んでいた。**式を引用するメッセージ全部**で
`x ^ flagA` が `x ? flagA` と出ていた。

そこにあった fixture は

```go
func main() {
    var x int = 1
    _ = x & 0
}
```

—— **演算子 1 つ、finding 1 件**。だからどちらの欠陥も見えなかった。
`fired` ≠ 検証済みが今週これで 3 度目である（goheader → forcetypeassert → SA4016）。

**ゲート**: fixture は SA4016 の 3 演算子すべてを両分岐に通す。`ok.go` には
黙るべきものを並べた —— `= iota` でなく 0 になる定数（syncthing の形）、
上流が declines する `pairA, pairB = iota, iota`、`= iota` だが 0 でない定数、
非ゼロのオペランド。`staticcheck-sa` の golden に 7 キー、ratchet の baseline は不変
（missing 3 / extra 1）。

---

### 2026-08-22（続き 18）— taint は 4 ルールで 1 エンジン、その節点集合は「全関数」ではない

続き 13 の「次にやること」4 と 5、そして続き 16 の教訓（health ゲートが hunt tier に無い）。
3 つのうち **2 つはゲートの欠落**で、コードの欠陥はそこから出てきた。

#### 1. gosec の未実装 taint —— G702 / G706 / G710 を足し、G703 を SSA に載せ替えた

上流 gosec v2.26.1 は `taint/taint.go` の**ひとつのエンジン**を 4 つの表で駆動する
（`analyzers/{commandinjection,pathtraversal,loginjection,openredirect}.go`）。
guff にあったのは G703 だけ、しかも**位置順再生の AST 近似**（続き 12）だった。
`crates/guff-style/src/gosec_taint.rs` にエンジンを SSA へ移植し、表を 4 本置いて
AST 版を削除した。移植して初めて分かったことが 3 つある。

**(a) source は 2 種類ある。** `os.Getenv` / `os.ReadFile` / `os.Args` は
**関数 source** で、呼べばそこが汚れる。`*http.Request` / `*url.URL` /
`url.Values` / `*bufio.Reader` / `*bufio.Scanner` は**型 source** で、
それ自体は何も汚さない —— **仮引数がその型のときだけ**汚れる。
`http.NewRequest` にハードコードした URL を渡しても source にならないのがこの区別の目的で、
上流のコメントもそう書いている。

**(b) 型 source の仮引数が汚れるかは call graph が決める。そして上流の call graph の
節点集合は `ssautil.AllFunctions` —— 「全関数」ではなく到達可能性の集合である。**
種は 3 つ: パッケージレベル関数、**exported** な型のメソッド、
`Program.RuntimeTypes()`（= どこかで interface に変換された型）のメソッド。したがって

```go
type rec struct{}
func (p *rec) log(id string)  { log.Printf("got %s", id) }        // 黙る
func (p *rec) serve(w http.ResponseWriter, r *http.Request) { p.log(r.URL.Path) }
```

`rec` は unexported で、どこでも interface に変換されていない。すると `serve` は
**節点ですらない**ので `log` に入る辺が無く、`id` は自分がリクエスト由来だと知る術がない。
`rec` を exported にするか `http.Handler` として登録するか（syncthing の
`crashReceiver` はこれ）した瞬間に、同じコードが撃つ。

**(c) 例外がひとつあり、実リポの findings の半分はそれである。** 型 source の
**仮引数のフィールド読み**は call graph に何も訊かずに汚れる
（`isFieldAccessTainted` の CASE 1）。syncthing の `rhost := r.RemoteAddr` がその形で、
G706 の 9 件のうち 4 件はこれ。

sanitizer（G703 の `filepath.Clean` / `path.Base` / `strconv.Atoi`、G706 の
`strings.ReplaceAll` / `strconv.Quote`、G710 の `url.QueryEscape`）と `CheckArgs`
（`http.Redirect` は URL だけ、`http.ServeFile` はパスだけ、`slog.Warn` は
**メッセージだけ** —— 属性値は両ハンドラがエスケープするため）は、AST 版に 1 つも無かった。

**可変長引数だけは意図的に違う。** go/ssa は可変長実引数を配列に詰めて `*ssa.Slice` を
1 つ渡すが、guff は個別に渡して `CallCommon::ellipsis` に記録する。全引数を見る sink では
答えが一致する（上流は配列の `Alloc` の referrer を辿って要素に届く）。`CheckArgs` を持つ
sink では添字がずれうるが、4 ルールが名指しする添字は**すべて固定引数**
（`slog.Warn` の `msg`、`http.Redirect` の `url`）なので問題にならない。

#### 2. 節点集合を合わせるのに、土台を 2 つ直した

移植した直後、syncthing の taint 15 件のうち 6 件しか出なかった。残りは全部 (b) である。

**`Program::runtime_types()` が推移的でなかった。** go/ssa の `needMethodsOf` は
boxed な型から**要素型・フィールド型・キー型・引数型・結果型・`*T`** へ再帰する ——
reflection から届く範囲が全部 runtime type だからである。guff は
`make_interface_types`（直接 box された型）をそのまま返していた。
syncthing の `(*serveCmd).monitorMain` はこの推移閉包でしか届かない: `serveCmd` 自体は
box されないが、それを保持する kong の CLI 構造体が box される。
`skip` フラグ（named の underlying は「訪れるが runtime type ではない」）ごと移植した。

**CHA の 3 種類目の辺が無かった。** `cha.CallGraph` は callee を 3 通りに解決する:
static callee、interface invoke（同名メソッド全部）、そして**関数型の値ごしの呼び出し**
（`funcsBySig` —— シグネチャが一致する bare 関数全部）。3 つ目は特殊ケースではない ——
syncthing の `unixConfigDir(…, fileExists)` は `os.Lstat` を**仮引数ごしに**呼ぶので、
この辺が無いと sink に届く経路が存在しない。

そのシグネチャ比較で 1 つ踏んだ: **Go の `Type.String()` は仮引数名を含む**ので、
`fileExists` 自身の型は `func(path string) bool`、それを受ける仮引数の型は
`func(string) bool` と描画され、同一なのに一致しない。`types.Identical` は名前を見ない
（`typeutil.Map` が使うのはそちら）。引数型と結果型と可変長フラグだけからキーを組み立てた。

**guff-ssa の差を 1 つ当て木した。** go/ssa は addressable な複合リテラルを**その場に**書く
（`t1 = &t0.cmd; *t1 = os.Getenv(…)`）が、guff は `complit` 一時変数に組んでから
構造体ごとコピーする。すると `h` 自身の referrer に**フィールド store が 1 つも無く**、
上流の walk（`FieldAddr` の store しか見ない）は何も見つけない。`stores_to_field` で
「構造体まるごとの store」を辿って元のセルに戻る形にした。**本来は `builder` を直すべき差**
だが、そこは全 SSA analyzer が共有しているので taint 側の当て木に留めてある。

#### 3. ゲートを足したら、その場で 1 パッケージ落ちていた

`compat/hunt.sh` に health ゲート（下記 5）を入れた最初の実行が、syncthing の
`lib/sliceutil` を ill-typed で落とした。中身は

```go
func RemoveAndZero[E any, S ~[]E](s S, i int) S {
	s[len(s)-1] = *new(E)          // invalid argument: s for built-in len
	return s[:len(s)-1]
}
```

続き 16 の `append` と**同じ族**で、`builtins.rs` の `len` / `cap` に
`// DEFERRED: type-parameter (Interface/underIs) operands.` が残っていた。
ただし訊くべき述語は `append` とは違う: `append` は `coreType`（＝ `commonUnder`）だが、
`len` は **`underIs`** —— 型集合の各項が個別に「長さを持つ」ことだけを要求する。
`~[]int | ~[]string` は共通の underlying を持たないが `len` は通る。
`crates/guff-types/tests/check_files.rs` に 2 本足した（通る 6 形と、
`~[]int | ~int` / `cap` on map / `any` の 3 つの拒否）。

そして baseline を記録するために hunt を 16 リポ回したら、**同じ形がもう 1 つ**出た。
rclone の `lib/atexit` と `fs/rc/jobs`、6 行に縮む:

```go
var fn *func()
func run() { (*fn)() }        // guff: p is not a type
```

`(*p)()` は**構文としてはポインタ変換 `(*T)(x)` と区別が付かない**ので、
`expr_or_type` はまず型として評価してみる。ところがその探りは**途中で報告してしまう** ——
値としての経路がそのあと正しく処理しても、"p is not a type" は残り、パッケージは ill-typed。
`builtin_new` が `new(x)` のために既に持っていた「探りの診断を巻き戻す」形
（`let mark = self.errors.len(); … self.errors.truncate(mark);`）を `expr_or_type` にも入れた。
テストは 2 本 —— 通るべき `(*fn)()` と、**変換の側が壊れていないこと**
（`(*T)(x)` は通り、`(*Nope)(x)` は**ちょうど 1 回**報告する）。

#### ゲート

- `crates/guff-style/tests/testdata/gosec/g7xx.go` —— 1 パッケージ、
  すべての関数に `// fires` / `// silent` を付けた。件数だけなら「全部撃つ」実装でも通るので、
  **撃つ形の隣に必ず黙る形を置いてある**: 同じ source に sanitizer を掛けたもの、
  sink が実際に見る引数だけ定数にしたもの、再代入で taint を殺したもの、そして (b) の
  **箱に入れた型と入れない型の、同じ 3 行**。
- `compat/golden/cases/gosec` が 127 キーを行・桁・severity ごと固定する
  （G702 は既存の `bad.go:61/62` にも 2 件増えた —— AST 版が一度も出せなかったもの）。
  `stub/` に `bufio` / `log` / `log/slog` / `net/url` / `strings` / `syscall` / `path` を足した。
- `checks_test.rs` に 2 本。片方は件数、もう片方は **(b) の対**だけを見る。

**syncthing**: taint の guff-only 0、golangci-only 15 → **0**。
P=98.3% / **R=93.2% → 95.6%**。残る gosec 差分は G705（XSS、5 本目の taint ルール・未実装）
×3、G402 の cipher suite ×1、G102 ×2、G115 の**文言**差（`rune -> byte` vs
`int32 -> uint8`）×2 —— どれも taint とは別件である。

#### 4. `uniq-by-line` の linter 順序 —— guff は合っていて、ゲートが無かった

続き 13 は「再走で差分が動いた形跡がある」と書いていた。上流の順序を実装から確定させた:

- `UniqByLine` は (file, line) ごとに**最初に届いた 1 件**を残す。`SortResults` は
  **全プロセッサの最後**なので、並べ替えは効かない。
- `Runner.Run` は linter を順に回して issues を append する。その順序は
  `GetOptimizedLinters` の並びだが、**2.12.2 では全 linter が 1 つの
  `goanalysis_metalinter` に入る**（`default: all` で「Combined 115」）。
  効くのは `combineGoAnalysisLinters` 内の並べ替えだけで、それは
  **名前順 + `nolintlint`（`linter.LastLinter`）が最後**。
- 上位リストの `DoesChangeTypes`（`unused` を末尾へ回す規則）は、その `unused` も
  metalinter の中にいるので**一度も適用されない**。

guff はこの順序を既に再現していた（`exclude.rs` の名前ソート＋
`NolintIndex::filter_issues` が nolintlint を末尾に回す）。**無かったのはゲートである** ——
`cases/issues-uniq-by-line` の衝突ペアは errcheck/gosec/govet/revive/staticcheck だけで、
**全部アルファベット順に一致する**ので、素の名前ソートでも通ってしまう。

`compat/golden/cases/issues-uniq-by-line-order` を新設した。1 パッケージ 1 ファイルで
（上限系の fixture と同じ理由 —— 到着順がレースする linter を混ぜない）、名前順と上流順が
**食い違う 2 組**を置く: revive vs nolintlint、unused vs nolintlint。対照として
errcheck vs gosec vs nolintlint を 1 行に並べた。fixture のコメントには
「exported 関数に doc コメントを付けてはいけない」と書いてある —— revive の `exported` は
コメントがあると**コメントの位置**に報告するので、finding が対象行から外れて
ケースが黙って無力化される。

そのうえで**リポ規模の比較は順序に依存させない**: `corpus/patch_unlimited_issues.py` に
`--uniq-by-line` を足し、`compat/hunt.sh` が `false` を渡す。両ツールが同じ config を読むので、
片側の 1 件の欠落が「どちらが残るか」を入れ替えて無関係な場所に差分を動かす形が消える。
OSS tier は allowlist が上流既定の下で記録されているので**触っていない**
（`compat/tests/test_patch_config.py` がその 2 つを両方とも表明する）。

#### 5. health ゲートが hunt tier に無かった

続き 16 の教訓そのもの。`compat/hunt.sh` に `GUFF_DEBUG_ILL_TYPED=1` と
`health.py check` を足し、`--update-baseline` を付けた。baseline は
`compat/baselines/health-hunt.json` に分けてある —— OSS 側は CI ゲートの記録なので、
hunt の refresh がそこを書き換えられる形にはしない。

**ゲートのゲート**も足した: `compat/tests/test_health.py::WiringTests` が、guff を実 Go に
向ける tier（`run.sh` / `hunt.sh` / `golden/run.sh`）**すべて**について
(a) `GUFF_DEBUG_ILL_TYPED=1` を渡すこと、(b) `health.py check` を呼ぶこと、
(c) 失敗を数えたうえで**実際に exit 1 すること**を表明する。
この gate の壊れ方は「何も起きない」なので、tier に配線し忘れる以外の壊れ方が無い。
そして実際、入れた最初の実行が上の §3 を落とした。

**記録された初回の baseline は 16 リポで 38 パッケージ**だった —— この 5 日間、
誰も見ていなかった量である。上の 2 つの型検査修正で **33**（10 リポ）まで落ちた:

| target | ill-typed | 主な内訳 |
|---|---|---|
| jaeger | 9 | 未調査 |
| gitea | 6 | 未調査（deref で 7 → 6） |
| thanos | 4 | 未調査 |
| argo-cd / prometheus | 3 / 3 | 未調査 |
| rclone | 2 | build tag 付きテストファイル（`PurgeTempUploads` / `MakeTestDirs` undefined） |
| cli / traefik | 2 / 2 | `cannot assign to struct{…}`、untyped → `interface{}` |
| authelia / dapr | 1 / 1 | 未調査 |

**残りは baseline に載っているので、あとは縮むだけである。** 続き 16 の教訓
（「ill-typed は差分に出ない」）に対する構造的な答えがこれで、リポを 1 本足したときに
**その場で数が出る**ようになった。

**次にやること**

1. ~~**hunt の ill-typed 33 件**~~ —— 続き 19 で 20 件まで落とした。残りは下記。
2. ~~**G705（XSS）**~~ —— 続き 24 で解消。syncthing の gcl-only 3 → 1
   （残る 1 件は表ではなく呼び出しグラフの半分）。ついでに authelia の
   nolintlint 偽陽性が 1 件消えた —— そのディレクティブのコメントが
   「TODO: Run this line through taint analysis」だった。
3. **G115 の文言** —— `rune -> byte` と `int32 -> uint8`。上流は宣言された別名で綴り、
   guff は basic kind で綴る。行も桁も一致するので、差分の両側に同時に並ぶ。
4. **複合リテラルの lowering** —— go/ssa は addressable な複合リテラルをその場に書く。
   guff の `complit` 一時変数は §2 の当て木を必要にしている唯一の理由で、
   直せば当て木は消える。全 SSA analyzer に効くので単独のタスクにすること。

---

### 2026-08-22（続き 19）— ill-typed 33 件を数え直したら、上位 2 クラスは 1 行ずつだった

続き 18 の「次にやること」1。baseline に載った 33 件を**エラーメッセージで分類**した
（`GUFF_DEBUG_ILL_TYPED=1` の出力をリポ横断で見るだけ）。5 クラスに割れて、
**上位 2 つは型検査の 1 行ずつ**だった。

| クラス | 件数 | 内容 |
|---|---|---|
| A `cannot index T[K, V any]` | 9 | 型引数 2 つ以上の**ジェネリック型への変換** |
| B `cannot assign to X (neither addressable …)` | 5 | ポインタ間接参照の addressability |
| C `undefined: pkg.X` | 7 | `export_test.go`（外部テストパッケージ） |
| D `cannot use *T value as I value` | 5 | 埋め込んだジェネリックインスタンスのメソッド昇格 |
| E その他 | 7 | 個別 |

#### A —— `T[A, B](v)` は変換であって呼び出しではない

```go
_ = iter.Seq2[[]ptrace.Traces, error](func(yield func([]ptrace.Traces, error) bool) { … })
```

`call_expr` の `IndexListExpr` の腕は**ジェネリック関数の明示的インスタンス化**しか
知らず、それ以外は "cannot index" にしていた。**型引数 1 つの形は通っていた** ——
そちらは `index_expr` を通り、そこがインスタンス化するため。だから
「2 つ以上のときだけ落ちる」形で、jaeger の 9 パッケージが全部この 1 行である。
基底が型に評価されたら `expr_or_type` に流す（探りの診断は続き 18 と同じく巻き戻す）。

#### B —— ポインタ間接参照は「常に」addressable

```go
*(*Sample)(ptr) = Sample{…}     // 変換の deref
*getPtrFunc(app) = test.fieldVal // 呼び出し結果の deref
```

`star_expr` は「被演算子が addressable なら結果も addressable」と書いていた。
spec は "a pointer indirection" を "a variable" と**並べて**挙げており、
go/types は `x.mode = variable` を**無条件に**セットする。thanos / argo-cd / cli の
5 パッケージがこれ。

**ゲート**: `check_files.rs` に 4 本。A は 1・2・3 引数の変換が通ることと、
**この腕が本来守っていたもの**（ジェネリック関数の明示的インスタンス化、部分的な
型引数、そして非ジェネリックへの多重添字は今も "cannot index"）。B は
変換・呼び出し・スライス要素・`(*s).V` の deref が通ることと、
**addressable でないものは今も拒む**こと（`mk().V = 1`、`m["k"].V = 2`）。

**hunt の ill-typed 33 → 20**（jaeger 9 → 0、thanos 4 → 2、argo-cd 3 → 2、cli 2 → 1）。

#### E の一部 —— 推論の「untyped」は「untyped *定数*」ではない

```go
opts.IsScopedRun = optional.Some(workflowSourceRepoID > 0)   // func Some[T any](v T) Option[T]
```

比較の結果は **untyped bool の「値」**であって定数ではない。guff は step 3
（untyped 実引数を既定型に昇格して型引数を決める）に渡す条件を
`mode == Constant` にしていたので、この実引数は step 1 からも step 3 からも落ち、
`T` が決まらなかった。Go が除くのは untyped **nil** だけ（既定型を持たないため）で、
step 1 の guard についてはコードのすぐ上のコメントが既にそう書いていた ——
**同じ読み違いが 2 行下に残っていた**。gitea の 3 パッケージがこれで、
`ill-typed 20 → 16`（gitea 6 → 2）。

#### E の残り —— untyped 定数を interface へ変換するのは「表現可能か」ではない

```go
ctxNew := context.WithValue(context.Background(), any("key"), "value")
_ = interface{}("bug-fix")
```

untyped 定数が interface として**表現可能**かを訊いても意味がない（どれも表現できない）。
Go は `implicitTypeAndValue` の `*Interface` の腕で**既定型**を通して変換し、
**空 interface のときだけ**受け入れる。guff の `assignable_to` は untyped の枝で
`representable` を訊いていたので、`any("key")` を拒んで gitea の `cmd/cmdtest` と
cli の `pkg/cmd/pr/list` を落としていた。

述語の選び方も同じくらい効いた: `Interface.Empty()` は `typeSet().IsAll()`
（**すべての型**の集合）であって、`is_empty()`（何も満たさない集合）ではない。
間違えたまま最初に直したときは `interface{}` リテラルだけが通った ——
そちらは型集合がまだ計算されていなかったからで、`MyAny` / `any` は計算済みで落ちた。
**`cached_typeset()` が `None` を「空」と読むコードは他にもある**（`predicates.rs:343`、
`check_expr_const.rs` の interface の腕）。触っていないが、同じ読み違いの候補である。

`ill-typed 16 → 14`（gitea 2 → 1、cli 1 → 0）。

**次にやること**（この 14 件の内訳）

1. **C: `export_test.go`（7 件）** —— `package storage_test` が import する
   `.../storage` は、**同パッケージの `_test.go` を含んだ test variant** でなければ
   ならない。guff は素のパッケージに解決している。authelia / dapr / gitea / thanos /
   rclone。`dedup::import_path_of_id` が `Q [P.test]` を `Q` に潰しているのは
   seed（production ファイルだけを compile する）には正しいが、
   `P [P.test]` —— P 自身の `_test.go` を含む variant —— には正しくない。
   コメント自身が「解析にとっては別パッケージである」と書いてある。
   **パッケージロード側の話で、型検査ではない**ので単独のタスクにすること。
2. **D: 埋め込んだジェネリックインスタンス（7 件）** ——
   `type ingressRoutes struct { *gentype.ClientWithListAndApply[A, B, C] }` の
   メソッド昇格。traefik / argo-cd / prometheus、いずれも k8s 系のコード生成物。
   **最小再現と、原因の絞り込みまではできている**（2026-08-22）:

   ```go
   // gentype パッケージ（別パッケージであること）
   type Client[T any] struct{ name string }
   func (c *Client[T]) Get(name string) (T, error) { var z T; return z, nil }

   // 利用側
   type noCtor struct{ *gentype.Client[*Route] }
   type OnlyGet interface{ Get(name string) (*Route, error) }
   func B() OnlyGet { return &noCtor{} }   // guff: cannot use *noCtor value as OnlyGet value
   ```

   **同じパッケージのどこかに 1 つでも「式の文脈」でそのインスタンスが現れると、
   パッケージ全体が直る。**

   | 追加した行 | 文脈 | 結果 |
   |---|---|---|
   | `gentype.NewClient[*Route]("r")` | 式 | **通る** |
   | `var _ = (*gentype.Client[*Route])(nil)` | 式（変換） | **通る** |
   | `var _ *gentype.Client[*Route]` | 型式 | 落ちる |

   つまり **型式の文脈で作られたインスタンスにはメソッドが無く、式の文脈のものには有る**。
   `Context` のインスタンスキャッシュは (origin, targs) で引くので、
   **先に作られたほうがパッケージ全体の答えを決める**。インスタンス側は
   `named_lookup_method` で origin を引く遅延方式なので、疑うべきは
   `typexpr::instantiated_type` の `generic_type(x)` が返す **origin そのもの** ——
   import が完全に解決される前の版を掴んでいる可能性が高い。
   importer とジェネリクスの境目の話なので単独のタスクにすること。
3. **E: 残り 2 件** —— gitea 1（未調査）と dapr 1（`export_test.go` 系）。

---

### 2026-08-23（続き 20）— 埋め込んだジェネリックインスタンス: 「別パッケージ」は縮約の副産物で、原因は 1 つ隣の関数だった

続き 19 の「次にやること」2。7 件のうち **5 件が 1 つの再帰の欠落**で、残り 2 件は
D ではなく C 系だった。そして**続き 19 が書いた絞り込みは、2 つのうち 1 つが間違っていた**。

#### 「別パッケージであること」は再現に要らなかった

続き 19 の 6 行の再現は `gentype` と利用側の 2 パッケージに分かれていて、
そこから「importer とジェネリクスの境目」「`generic_type(x)` が返す origin が
import 解決前の版」という筋を立てていた。**同じ 6 行を 1 パッケージに貼ると、
同じ 1 行で同じように落ちる。**

```go
package p
type Route struct{ N int }
type Client[T any] struct{ name string }
func (c *Client[T]) Get(name string) (T, error) { var z T; return z, nil }
type OnlyGet interface{ Get(name string) (*Route, error) }
type noCtor struct{ *Client[*Route] }
func a() OnlyGet { return &noCtor{} }   // guff: cannot use *noCtor value as OnlyGet value
```

分けたのは縮約の途中でそうなっただけで、importer は一度も関与していない。
**縮約が保った構造と、縮約が保った偶然を、区別する手順が要る** ——
ここでは「境界を 1 つ消してもまだ落ちるか」を 1 度訊くだけで済んだ。
続き 19 のもう一方の観察（下の表）は正しく、そちらだけで原因に届く。

#### 原因 —— 展開の再帰が、解決の再帰と食い違っていた

`Checker::prepare_method_set` は 2 段で、interface 充足を訊くすべての入口が通る:

1. `ensure_method_sigs` —— メソッドのシグネチャを `obj_decl` で解決する。
   **埋め込みフィールドを降りる**（`ensure_method_sigs_rec`）。
2. `expand_instance_methods` —— インスタンスのメソッド表を、型引数を代入した
   コピーで埋める。**降りていなかった。**

`noCtor` 自身はインスタンスではないので 2 は即 return し、埋め込まれた
`Client[*Route]` は空のまま残る。すると `named_lookup_method` は
「インスタンスにメソッドが無ければ origin を引く」経路に落ち、
`lookup_field_or_method` は **origin の `Get`** ——
`func(string) (T, error)`、`T` は `Client` 自身の型パラメータのまま —— を昇格する。
`Get(string) (*Route, error)` と比べれば当然合わない。

1 の側には**まったく同じ形のコメントが既に付いていた**（kubernetes の
`apimachinery/pkg/api/meta` から縮約した `struct{ Multi }`、Phase 4）:
「メソッド集合は自分のメソッドだけではない」。**対になる 2 つの歩き方のうち、
片方だけが直された**というのがこのバグの正体で、`prepare_method_set` の
2 行を並べて読めば見えるところにあった。

そして続き 19 の表は、これで全部説明が付く:

| 追加した行 | 何が起きるか | 結果 |
|---|---|---|
| `NewClient[*Route]("r")` | `*Client[*Route]` **自体**に interface 検査が走り、共有インスタンスが展開される | 通る |
| `(*Client[*Route])(nil)` | 同上（変換の充足検査） | 通る |
| `var _ *Client[*Route]` | 型式。interface 検査が無いので誰も展開しない | 落ちる |

「型式で作られたインスタンスにはメソッドが無い」のではなく、
**誰かが一度でも展開すれば、以後そのインスタンスは全員に対して正しい**。
インスタンスが (origin, targs) で共有されているという続き 19 の観察は正しく、
向きが逆だっただけである。

#### 直し方

`expand_instance_methods` を `ensure_method_sigs_rec` と同じ形の再帰にした ——
`deref` して `seen` に入れ、自分がインスタンスなら展開し、underlying が struct なら
埋め込みフィールドを降りる。**2 段の順序は保つ**: 解決の歩きを最後まで済ませてから
展開の歩きを始める。展開は一度きり（`num_methods() > 0` で降りる）なので、
未解決のシグネチャを掴んだまま展開すると**永久に直らない**からである
（`prepare_method_set` のコメントが既にそう書いている）。

ただし **`check_assign::assignable_to` はこの 2 つを逆順に呼んでいる**
（`expand_instance_methods` → `ensure_method_sigs`）。今回そこが壊れないのは、
`expand_one_method` が origin のメソッドに**自分で `obj_decl` を掛けてから**
シグネチャを読むからで、順序の不変条件は 1 箇所でしか守られていない。
埋め込みを降りるようになった分だけこの当てにしている範囲は広がったので、
`prepare_method_set` に寄せるのは別タスクとして残しておく。

#### ゲート

`check_files.rs` に 3 本。

- **`embedded_generic_instance_promotes_substituted_methods`** —— 撃つ形と黙る形を
  **別々の `check_src` に分けてある**。インスタンスは (origin, targs) で共有されるので、
  同じパッケージに `NewClient[*Route]("r")` を 1 行置くと**それが被験体を治してしまう**
  —— 1 パッケージに両方入れた最初の版は、修正前のコードでも通った。
  昇格したメソッドの**結果型**も見る（`func b(n *wrap) *Route { r, _ := n.Get("x"); return r }`）:
  代入が通ることだけを見ていると、`T` のままでも通る形が残る。
- **`..._promotes_through_nested_embedding`** —— 型引数 3 つ、埋め込み 2 段
  （k8s の `ClientWithListAndApply` の形）、各段が 1 メソッドずつ出す。
- **`..._still_reports_a_method_set_that_does_not_match`** —— 展開が
  「昇格したメソッドなら何でも通る」に化けていないこと。落ちる理由が 4 つとも違う:
  型引数違い（`Client[*Other]`）、**値**埋め込みごしのポインタレシーバ、
  そもそも無いメソッド、そして昇格結果が `*Route` であること（`*Other` に代入して落ちる）。
  エラーは**ちょうど 4 件**を要求する。この 3 本目は修正の前後どちらでも通る ——
  それがこの 1 本の役目である。

**hunt の ill-typed 14 → 9**: traefik 2 → 0、argo-cd 2 → 0、prometheus 3 → 2。
続き 19 が D に数えた 7 件のうち**実際に D だったのは 5 件**で、prometheus に残る 2 つは
どちらも `cannot use invalid type value as *promql.Engine value`。片方は
`promql_test` そのもの（＝クラス C）で、もう片方の `cmd/promtool` は
**`./cmd/promtool` だけ、あるいは `./cmd/promtool ./promql` の 2 つだけをロードすると落ちない**
—— モジュール全体をロードしたときだけ `promql.Engine` が invalid になるので、
C と同じ test variant の潰れが上流にいる疑いが濃い。C を潰すときに一緒に確かめること。

**残る 9 件の内訳**（baseline 更新後の `GUFF_DEBUG_ILL_TYPED=1` から）

| パッケージ | クラス |
|---|---|
| gitea `modules/markup_test` | C |
| authelia `internal/storage_test` | C |
| dapr `pkg/diagnostics_test` | C |
| prometheus `promql_test` | C |
| rclone `backend/cache_test` / `backend/union_test` | C |
| thanos `internal/cortex/chunk/cache_test` | C |
| prometheus `cmd/promtool` | C の巻き添えと推定（上記） |
| thanos `pkg/promclient` | E（下記） |

**7 件がきれいに `_test` で終わる** —— C を潰せば残りは 2 件になる。

**次にやること**

1. ~~**C: `export_test.go` / 外部テストパッケージ（7 件）**~~ —— 続き 21 で 6 件を消化
   （ill-typed 9 → 3）。**筋書きは外れていた**: `dedup::import_path_of_id` は無罪で、
   seed が依存を production ファイルだけで組んでいたのが原因。
   prometheus の 2 件はそもそも C ではなく、seed の wave スケジューラの欠陥だった。
2. ~~**G705（XSS）**~~ —— 続き 24 で解消（syncthing の gcl-only 3 → 1）。
3. **G115 の文言** —— 続き 18 の 3。
4. **複合リテラルの lowering** —— 続き 18 の 4。
5. **`assignable_to` の 2 段の順序** —— 上記のとおり `prepare_method_set` と逆。
   今は `expand_one_method` の `obj_decl` が支えているだけなので、寄せる。
6. ~~**E: 浮動小数定数の整数への変換**~~ —— 続き 22 で解消（hunt の ill-typed 9 → 8）。
   **疑いは外れていた**: guff は「小数点を持つ float 定数の変換をまとめて拒んで」は
   いない（`int64(1.5*hour)` は通る）。落ちていたのは**演算のたびの丸めが無かった**
   ためで、型付き定数どうしの積が厳密な有理数のまま残っていた。

---

---

### 2026-08-23（続き 21）— `export_test.go` は「別のパッケージ」ではなく「同じパッケージの広い版」だった

続き 20 の「次にやること」1。C の 7 件のうち **6 件が seed の 1 つの条件**で片付き、
残る prometheus の 2 件は **C ではなかった**。そして続き 19 が名指しした
`dedup::import_path_of_id` は**無罪**である —— あれは `Q [P.test]` を `Q` に潰す関数で、
潰していること自体は正しい。潰した先で**どのファイルを読むか**が違っていた。

#### 原因 —— seed は依存を production ファイルだけで組む

`package p_test` は P のテストバイナリに入るので、そこでの `import ".../p"` が指すのは
**test variant `P [P.test]`** —— P 自身のファイル**＋P の同一パッケージ `_test.go`** である。
`export_test.go` の存在意義はその variant を広げることそのものなので、
production ファイルだけで組んだ P には広げた分が無い:

```go
// p/export_test.go —— package p。T の非公開フィールドに触るので p にしか置けない
func Reveal(t T) int { return t.x }

// p/ext_test.go —— package p_test
p.Reveal(p.New(7))        // guff: undefined: p.Reveal
```

`build_source_seed_inner` は依存の `compiled_go_files` から `_test.go` を**無条件に**
落としていた（3 箇所）。**その判断自体は正しい** —— 全依存に効かせると
prometheus `./...` で型アリーナの RSS がおよそ 2 倍になる、とコメントが測って書いてある。
足りなかったのは条件で、**その load に P の外部テストパッケージが居るときだけ**
P の seed を `P [P.test]` のファイルで組む。

#### 外部テストパッケージの見分け方は括弧ではない

`go list -test` の id は 3 種類が同じ括弧を着る。

| id | 中身 |
|---|---|
| `P [P.test]` | P 自身のテスト variant（production + 同一パッケージ `_test.go`） |
| `Q [P.test]` | **別の**パッケージ Q を P のテストバイナリ向けに再コンパイルしたもの |
| `P_test [P.test]` | **外部テストパッケージ**（`package p_test` のファイル） |

見分けるのは括弧ではなく「自分の path が P に `_test` を足したものか」。
`external_test_package_under_test` がこれで、`paths_with_external_test_package` が
load 全体からその P を集める。**これがゲートの全部**であり、広げすぎないための唯一の仕掛けである。

#### seed のファイルと seed の辺は同じ variant から採る

`import_path_dep_graph` は「plain `id == pkg_path` を優先」だった。
P を test variant で組むなら辺もそちらから採らないと、`_test.go` の import
（`testing`、testify …）が graph に載らず、seed の中で解決先を失う。同じ集合を両方が読む。

**縮約された seed に P は 1 つしか置けない**ので、広げた P は `q` のような production の
importer からも見える。これは意図的な選択で、`q.Describe(t p.T)` に外部テストパッケージが
自分の `p.T` を渡す形が通るのは、両者が**同じ** `p.T` を見ているからである。
代償は、production のコードが test 限定の識別子に触っても guff が型エラーにしないこと ——
そのコードは `go build` が通らないので実在しない。

#### 結果

**hunt の ill-typed 9 → 3**: authelia 1 → 0、dapr 1 → 0、gitea 1 → 0、rclone 2 → 0、
thanos 2 → 1（`internal/cortex/chunk/cache_test` が消え、残るのは E の `pkg/promclient`）。
`compat/baselines/health-hunt.json` は 6 行から **prometheus 2 / thanos 1 の 2 行**になった。

**差分は 1 件も増えていない。** 同じコーパス・同じ golangci-lint 2.12.2 で、
続き 20 の hunt（`hunt-20260822T165933Z`）と今回（`…T225919Z`）の 16 ターゲットを
突き合わせると、guff / golangci / both が動いたのは syncthing だけで、
そこも**上流側**が 654 → 656 に増えて guff が既に持っていた 2 件と一致した
（unexpected 42 → 40）。ill-typed だった 6 パッケージは、
分析されるようになっても**どちらのツールも何も報告しない**中身だった ——
つまり今回の回収は finding ではなく、**黙って落ちる面積**のほうである。

**コストは測れない。** prometheus `./...` を base / fixed 交互に 5 回ずつ:
real 2.03–2.20 / 1.99–2.21 秒、user 8.83–9.60 / 8.86–9.60 秒、
maxRSS 1828–1858 / 1818–1853 MB。広げるのは外部テストパッケージを持つ path だけで、
seed の依存は `ignore_func_bodies` で組むので、増える `_test.go` は宣言しか置いていかない。

#### ゲート

`crates/guff-packages/tests/external_test_package_seed.rs` に 5 本。
`go` も export data も要らない手組みの `Package` と、`tests/testdata/xtest` の実ファイル。

- **`external_test_package_sees_export_test_symbols`** —— 修正そのもの。
  dedup 前（plain `P` が居る）と dedup 後（`P [P.test]` だけ）の**両方の形**で回す。
  被験体は `p.Reveal` だけでなく `q.Describe(v)` も呼ぶ: P を「production の写しと
  test variant の写し」に割る実装は `undefined:` を消したうえでこちらで落ちる。
- **`a_package_without_an_external_test_package_is_seeded_production_only`** ——
  広げすぎの検出。`r` は同一パッケージ `_test.go` を持つが外部テストパッケージを持たないので、
  `user` から `r.Hidden` は今も `undefined`。**一番安い直し方（dedup を生き残った
  パッケージのファイルをそのまま seed に使う）はこの 1 本だけが落とす。**
- **`a_production_importer_of_an_augmented_package_still_checks`** —— `q` は素通り。
- **`only_p_test_brackets_p_names_an_external_test_package`** —— 上の表そのもの。
- **`augmented_paths_take_their_edges_from_the_test_variant`** —— ファイルと辺の出所が一致すること。

**両方向で確かめた**: `paths_with_external_test_package` を空集合にすると 1 と 5 が
`undefined: p.Reveal` で落ち、逆に「dedup を生き残った全部」に広げると 2 と 5 が落ちる。

#### prometheus の 2 件は C ではなく、seed の wave スケジューラの欠陥だった

`promql_test` と `cmd/promtool` はどちらも
`cannot use invalid type value as *promql.Engine value`。続き 20 は promtool を
「C の巻き添えと推定」と書いていたが、両方とも C ではない。
`GUFF_DEBUG_SEED_ERRORS=1` が一行で答えを出す:

```
guff:     seed dep github.com/prometheus/prometheus/promql/promqltest — 19 error(s), first: undefined: promql
```

`promqltest` が seed で型検査されたとき `promql` がまだ merge されていない。
`NewTestEngine` の戻り型 `*promql.Engine` が invalid になり、
それを使う**2 つ隣のパッケージ**が ill-typed になる。原因は 3 段:

1. `import_path_dep_graph` は辺を**括弧を剥がして**張る（`Q [P.test] -> Q`）。
   これで **Go では非巡回な組が巡回になる**: prometheus では
   `util/teststorage` の test variant が `tsdb` に依存し、
   `tsdb` の test variant が `util/teststorage` に依存する。
   dedup が plain を両方落とすので、**どちらの key も test variant から辺を採る**。
2. `dep_load_order` の `visiting` ガードが片方の辺を黙って落として walk を終える。
   `order` はもう位相順ではない。
3. `height` のパス 2 はその `order` を信じているので、
   **まだ訪れていない consumer を持つパッケージの height を読む**。
   prometheus `./...` では back-edge 5 本に対して
   **`wave(dep) < wave(consumer)` を破る辺が 16 本**でき、
   その 1 本が `promql/promqltest -> promql`（どちらも wave 12）である。

**素直な直し（`order` の中で後ろを向く辺だけで伝播する）は試して捨てた。**
`promql` は正しく分かれた（wave 12 / 15、破る辺 16 → 0）が、そのとき落ちる辺は
`teststorage -> tsdb` の**実在する方**なので、`teststorage` が `tsdb` より先に組まれ、
`promtool` / `web` / `web/api/v1` / `promqltest` の 4 パッケージが
`teststorage.TestStorage has no field or method Dir` で ill-typed になる。
**1 件直して 4 件壊す。** 巡回が偽物である以上、直すべきはスケジューラではない ——
**seed が production ファイルを組むなら production の辺を渡す**、つまり
パッケージロード側である。`typecheck.rs` の当該箇所にこの経緯をコメントで残した。

**次にやること**

1. **seed の辺を「組むファイル」に合わせる（上記）** —— 非 augment の path では
   plain `P` の deps を使う。難所は供給源で、`filter_duplicate_packages` が plain を
   `all` から落とした後なのでもう手元に無く、`go list -test ./...` は
   for-test dep variant（`Q [P.test]`）を**トップレベルには出さない**
   （Deps の文字列の中にしか現れない）。**パッケージロード側の設計変更**になるので
   単独のタスクにすること。再現は prometheus `./...` の 2 件、
   観測は `GUFF_DEBUG_SEED_ERRORS=1` の 1 行、
   検算は `dep_graph` を Python で組み直して破る辺を数えるだけで足りる。
2. **G705（XSS）** —— 続き 18 の 2。`Receiver` sink と `ArgTypeGuards`。syncthing に 3 件。
3. **G115 の文言** —— 続き 18 の 3。syncthing の `deviceid.go:191/209` に今も両側で並ぶ。
4. **複合リテラルの lowering** —— 続き 18 の 4。
5. **`assignable_to` の 2 段の順序** —— 続き 20 の 5。`prepare_method_set` に寄せる。
6. **E: 浮動小数定数の整数への変換** —— thanos `pkg/promclient` の 1 行（続き 20 の 6）。
   **hunt に残る ill-typed 3 件のうちの 1 件**で、他の 2 件は上記 1。

---

### 2026-08-23（続き 22）— 型付き定数は「使うとき」ではなく「演算のたび」に丸められる

続き 20 の「次にやること」6（クラス E）。thanos `pkg/promclient` の 1 行。

```go
testutil.Equals(t, int64(2*time.Hour), int64(flags.TSDBMinTime))          // 通る
testutil.Equals(t, int64(4.8*float64(time.Hour)), int64(flags.TSDBMaxTime)) // guff: cannot convert float64 to type int64
```

#### 「4.8 だけが落ちる」は係数の話であって、型の話ではないように見えた

| 式 | 修正前 |
|---|---|
| `int64(2.0 * float64(time.Hour))` | 通る |
| `int64(float64(time.Hour) * 1.5)` | 通る |
| `int64(4.8 * float64(time.Hour))` | **落ちる** |
| `int64(float64(time.Hour) * 4.8)` | **落ちる** |
| `int64(4.8 * 3600000000000.0)` | 通る（untyped どうし） |

分かれ目は**係数が 2 進で正確に表せるか**である。1.5 と 2.0 は表せる。4.8 は表せない。
そして最後の行が効いていて、**同じ 4.8 でも untyped どうしなら通る** ——
つまり問題は乗算でも定数の値でもなく、**型が付いていること**にある。

#### 原因 —— 4 か所の `// DEFERRED: check.overflow(x)`

Go の規則は「型付き定数はその型で表現可能でなければならない —— **演算のたびに**」で、
go/types は `constant.BinaryOp` の直後に `check.overflow` を呼び、
そこから `representable` が値を**その場で丸め直す**。
guff は `basic_lit` / `unary` / `binary` / `shift` の 4 か所すべてに
`// DEFERRED: check.overflow(x)` を置いたまま出荷していた。

丸めないと、float64 定数どうしの積は**厳密な有理数**のまま残る。
4.8 の float64 表現は 5404319552844595/2^50 なので、

```
4.8 * 3.6e12 = 2374945115996159912109375 / 2^37     ← 整数ではない → int64 は truncation
round_float64(それ) = 17280000000000                ← 整数 → int64 は通る
```

untyped どうしなら丸める型が無いので厳密な 24/5 × 3.6e12 = 17280000000000 のまま通る。
**同じ係数が、型が付いた瞬間にだけ問題になる**のはこのためで、
「1.5 は通って 4.8 は落ちる」という見え方は原因ではなく症状だった。

#### `overflow` は 2 つのことをする

1. **型付き定数** —— `representable` で丸める（表現できなければその場でエラー）。
2. **untyped 整数** —— Go の定数精度 512 bit を超えたらエラーにして Unknown にする。

2 は副産物で、`const big = 1 << 1000` を guff は**黙って受けていた**。
文言は上流と 1 文字違わない `constant shift overflow`
（`opName` が入れる演算子の語まで含めて一致する）。

#### ゲート

`check_files.rs` に 5 本。**両方向で確かめた** ——
`overflow` を早期 return にすると 3 本が落ち、
「丸めをやめる」「丸めを広げる」のどちらに転んでも残りの 2 本が捕まえる。

- **`a_typed_float_constant_is_rounded_after_each_operation`** —— 修正そのもの。
  上の表の 5 行をそのまま入れてある（通っていた 3 行も含む）。
- **`rounding_neither_manufactures_nor_destroys_representability`** ——
  `int64(4.7 * hour / 7.0)` は float64 で本当に小数なので**今も落ちる**。
  丸めを結果側に効かせて整数に寄せる実装は 1 本目を通してここで落ちる。
  同じテストの後半で untyped の側（丸める型が無い）も押さえる。
- **`a_typed_constant_that_overflows_its_type_is_rejected_at_the_operation`** ——
  `const x int8 = 100; const y = x * 2` が **`x * 2` の位置で** 1 件。
- **`an_untyped_integer_constant_may_not_grow_past_the_constant_precision`** ——
  `1 << 1000` は落ち、`1 << 511`（ちょうど 512 bit）は通る。上限は `>` であって `>=` ではない。
- **`complementing_a_typed_unsigned_constant_survives_the_round`** ——
  `^uint(0)` は**型付き**なので新しく丸めを通る。符号付きに丸めれば `-1` が戻り、
  `1 << (^uint(0) >> 63)`（runtime 自身のイディオム）が `u64::MAX` ビットのシフトになる。
  `unary` のマスク処理のコメントが警告しているのがこの経路である。

#### 結果

**hunt の ill-typed 9 → 8**（thanos 2 → 1。`pkg/promclient` が消え、
残るのは `internal/cortex/chunk/cache_test` ＝ クラス C）。
**差分は 1 件も動いていない**: 16 ターゲットすべてで guff / golangci / both / unexpected が
続き 20 の hunt（`hunt-20260822T165933Z`）と完全一致。golden 103/103、isolate 116/116。
定数を丸めることは、どの check が読む値も変えなかった。

> **注意（マージ順）**: 続き 21（外部テストパッケージ）が thanos の `cache_test` を
> 潰すので、**両方が main に入った時点で thanos は 0 になり、
> `compat/baselines/health-hunt.json` の thanos 行は消してよい**。
> どちらの PR も単独では 1 までしか下げられないので、両方とも 1 を記録している。
> 減るのは自由なのでゲートは緑のままだが、行は古くなる。

**次にやること**

1. **seed の辺を「組むファイル」に合わせる** —— 続き 21 の 1。prometheus の 2 件。
2. **G705（XSS）** —— 続き 18 の 2。syncthing に 3 件。
3. **G115 の文言** —— 続き 18 の 3。`rune -> byte` と `int32 -> uint8`。
   **上流は `byte` / `rune` を「名前の違う別の `*Basic`」として持っている**
   （`go/types` の `aliases` 配列）のに対し、guff の `byte` / `rune` は
   `uint8` / `int32` と**同じ TypeId を指す TypeName** なので、
   「ソースが `byte` と書いた」という情報が型の側に残っていない。
   `basic.rs` の `BYTE` / `RUNE` は「同じ数値の別名」というコメントつきで
   そう決めてあるので、直すならモデルを変える話になる。単独のタスクにすること。
4. **複合リテラルの lowering** —— 続き 18 の 4。
5. **`assignable_to` の 2 段の順序** —— 続き 20 の 5。

---

### 2026-08-23（続き 23）— メソッドの名前を作れる関数が 1 つも共有されておらず、5 か所が同じ回避を書いていた

syncthing の gcl-only を linter 別に数えると errchkjson が 7 件で 2 番目に大きい。
**7 件とも `(*encoding/json.Encoder).Encode`** で、`json.Marshal` のほうは 4/4 一致していた
（`errchkjson: guff 4 / golangci 11 / both 4 → P 100% R 36.4%`）。

#### 原因 —— `call_name` はメソッドに対して Go が作らない名前を返す

上流 errchkjson は `types.Func.FullName()` で分岐するので、表はこう書かれている:

```go
case "encoding/json.Marshal", "encoding/json.MarshalIndent": …
case "(*encoding/json.Encoder).Encode": …
```

guff の `marshal_fn_name` は `code::call_name` を使っていた。
`call_name` は最後に `func_name`（**パッケージパス + オブジェクト名**）へ落ちるので、
メソッドは `encoding/json.Encode` になる —— **Go が決して作らない綴り**であり、
上流のどの表にも載っていない文字列である。だから Encoder の腕は一度も一致せず、
`Marshal` の腕（パッケージ関数なので同じ綴り）だけが動いていた。
**linter は生きているように見え、コーパスからは Encode が丸ごと消えていた。**

```
call_name        = Some("encoding/json.Encode")             ← Go が作らない
callee_full_name = Some("(*encoding/json.Encoder).Encode")  ← 表が書いてある綴り
```

#### 同じ穴を 5 か所が別々に埋めていた

`call_name` は doc コメントで「fully-qualified name」と名乗っているが、
メソッドについてはそうではない。そして**それを必要とした移植は全部、横で作り直していた**:

| 場所 | やっていること |
|---|---|
| `noctx::full_call_name` | `call_name` を呼んでから、同じ obj を引き直して `type_func_name` を取る |
| `errcheck`（`lib.rs:451`） | `call_name` の結果と `type_func_name` の結果を**両方** names に積む |
| `musttag::callee_name` | `call_name` を使わず最初から `type_func_name` |
| `govet waitgroup` | `is_call_to(…, "(*sync.WaitGroup).Add")` が**死んでいて**、その下に「セレクタが `Add` で受け手が `sync.WaitGroup`」の 3 行が置いてある |
| `staticcheck SA2000` | 同上（同じ 3 行がコピーされている） |

後ろの 2 つは**フォールバックが救っているので緑のまま**で、
死んでいる 1 行があることは誰にも見えない。errchkjson だけがフォールバックを書かなかった。

`code::callee_full_name`（＝ `Func.FullName()`）を足して errchkjson をそれに向けた。
**`call_name` 自体は触っていない** —— 15 前後の analyzer が結果を文字列として使っており
（`printf` は `rsplit('.')`、`wrapcheck` は `format!("{n}(")` して設定パターンと突き合わせる、など）、
中央で直すなら**その全部を読む**必要がある。下記「次にやること」に分けた。

#### isolate の fixture が 1 件しか比較していなかった

`compat/isolate/fixtures/errchkjson/bad.go` は `json.Marshal` 1 件だけで、
上流も guff も 1/1 で一致していた。§1 が数えた「**72 linter が 1 件だけ比較している**」
の一例がそのまま今回の穴を通した形になる。
呼び出しの 3 形（式文・blank 代入・変数レシーバ）と unsafe 型、
それに**黙るべき 2 形**（`return` と `if err :=`）を足して、上流の答えは 1 → **5 件**になった。

#### 結果

syncthing の errchkjson は **4/11 → 11/11、R 36.4% → 100%**（gcl-only 7 → 0）。
syncthing の unexpected は **42 → 33**、他の 15 ターゲットは 1 件も動いていない。

#### ゲート

- `compat/isolate/fixtures/errchkjson/bad.go` —— 上記のとおり 5 件。**上流と実行比較される。**
- `crates/guff-error/tests/testdata/errchkjson/encoder.go` +
  `errchkjson_flags_unchecked_encoder_encode` —— 4 件ちょうどを要求する
  （黙るべき 2 形を数に含めない形で書いてある）。**両方向で確認**:
  `callee_full_name` を `call_name` に戻すと 0 件になって落ちる。

**次にやること**

1. **`call_name` をメソッドについても `Func.FullName()` にする（中央の修正）** ——
   `is_call_to` / `is_call_to_any` に渡っている文字列リテラルは 34 種で、
   **`(*sync.WaitGroup).Add` 以外は全部パッケージ関数か組み込み**なので、
   直しても比較の意味は変わらず、死んでいる 1 行が生き返って
   上の 3 行のフォールバックが 2 か所で消せる。危険なのは
   **結果を文字列として使っている側**で、`printf.rs`（3 か所）/ `wrapcheck.rs` /
   `bodyclose.rs` / `st1013.rs` / `inline.rs` / `copylocks.rs` / `atomic.rs` ×2 /
   `defers.rs` / `sa6005.rs` を読む必要がある。単独のタスクにすること。
2. **他の linter にも同じ穴が無いか** —— 判定基準は
   「上流が `FullName()` で分岐しているか」であって「メソッドを扱うか」ではない。
   `grep -rn 'call_name' crates/` の結果を 1 つずつ上流と突き合わせる。

---

### 2026-08-23（続き 24）— G705: 上流の表に載っている sink のうち 4 つは、上流でも一度も撃てない

続き 18 の「次にやること」2。syncthing の gcl-only に G705 が 3 件。
コメントは「これだけは `Receiver` sink と `ArgTypeGuards` が要る」と書いてあり、
その 2 つが実際に何なのかを最初に確かめた。

#### 上流のソースがローカルにある

`~/projects/src/github.com/securego/gosec`（v2.27.1-9-g8495706）に checkout がある。
**表を推測せずに移植できる**ので、まず `analyzers/xss.go` と `taint/taint.go` を読んだ。
必要だったのは 2 つの機構だけで、残りは既存のエンジンがそのまま使える。

| 機構 | 何が違うか |
|---|---|
| `Receiver` sink | `(net/http.ResponseWriter).Write` は**インターフェースのメソッド**なので、SSA では invoke であり **static callee が無い**。`static_callee` しか見ない matcher は 1 件も見つけられない |
| `ArgTypeGuards` | `fmt.Fprintf` は「**HTTP レスポンスに書くとき**だけ」sink。これが無いと、web サーバの `Fprintf(os.Stderr, …)` がほぼ全部 finding になる |

#### 先に上流の答えを 14 形について取った

移植する前に、撃つ形・黙る形を 14 個並べたモジュールを golangci-lint に食わせた。
**7 fires / 7 silent**。これが仕様書になり、そのまま fixture の骨格になった。

その中で 1 つ、表を読んだだけでは出てこない事実が出た:

```go
func templateHTML(r *http.Request) template.HTML {
	return template.HTML(r.FormValue("q"))   // G705: 黙る
}
```

`xss.go` の表には `html/template` の `HTML` / `HTMLAttr` / `JS` / `CSS` が
sink として並んでいる。しかし**これらは型変換であって呼び出しではない**ので、
`analyzeFunctionSinks` が見る `*ssa.Call` には決してならない。
**上流でも一度も撃てない 4 行**である。表には忠実に入れたうえで、
fixture に `// silent` の実例として置き、コメントにそう書いた。
なお**その行が無警戒なわけではない**: 同じ形を AST 側の **G203** が撃っていて、
golden がその 1 件を pin している。

もう 1 つ、`resolveOriginalType` が飾りではないこと:

```go
var out io.Writer = w                       // w は http.ResponseWriter
fmt.Fprintf(out, "<p>%s</p>", r.FormValue("q"))   // 撃つ
```

sink に届く時点で writer は `io.Writer` に広がっているので、
**`io.Writer` が `ResponseWriter` を実装するか**を訊くと必ず no になる。
インターフェース変換を遡って「呼び出し側が何を渡したか」に戻す必要がある。

#### stub の `ResponseWriter` が `interface{}` だった

`crates/guff-style/tests/testdata/gosec/stub/net/http/http.go` の
`type ResponseWriter interface{}` は**空インターフェース**で、
つまり**あらゆる型が実装している**。この stub のままなら
`ArgTypeGuards` は単体テストの中で**常に真になる no-op** で、
「guard が効いている」ことを一度も確かめられなかった。
3 メソッドの本物にした。ついでに **`os.Stderr` が stub に存在しなかった**ので、
最初の実行では guard が invalid 型を訊いていて 9 件（正解 7 件）出た ——
**stub の穴が、guard のバグと同じ顔で出る。**
`*os.File` には `Write` だけを持たせてある: `io.Writer` ではあるが
`http.ResponseWriter` ではない、という差が G705 の guard そのものだからである。

#### ゲート

- `compat/golden/cases/gosec` —— includes に G705 を足し、上流から再生成。
  **142 キー**を `path:line:col:linter:severity:text` で正規化なしに比較する。
  G705 の 8 件は severity `medium`（taint は `RuleInfo.Severity`、confidence は常に High）。
- `crates/guff-style/tests/testdata/gosec/g7xx.go` に **8 fires / 10 silent**。
  silent の中身が本体である:
  - 4 つは guard で落ちる `fmt` / `io` 呼び出し。**guard を消すと finding になる。**
  - 2 つは**他のルールの source**（`*url.URL` と `os.Getenv`）。どちらも
    G703 / G706 / G710 では source で、G705 では source ではない ——
    5 つの表を 1 つに畳んだ実装はここで落ちる。同じ fixture の 40 行上で
    `r.URL.Path` が G703 と G706 を撃っているので、対比がその場にある。
    その隣に `os.Args`（G705 の source **である**ほう）を置いてある。
- `gosec_g705_needs_the_invoke_sink_and_the_writer_guard` ——
  invoke sink と guard を「落とすと何が壊れるか」で書いた 1 本。
- 既存の 4 ルールの本数 (7, 5, 5, 2) は動いていない。

#### 上流との差分実測

移植の途中で 3 セット、計 **32 形**を golangci-lint と 1 件ずつ突き合わせた:
14 形の sink/silent（上記）、**12 の sanitizer 全部**（両ツールとも 0 件）、
**6 の source**（`os.Args` / `bufio.Scanner` / `bufio.Reader` は撃ち、
`*url.URL` / `os.Getenv` / 素の `string` 引数は黙る）。
すべて行・列まで一致。

#### 一つだけ上流と意図的に変えた

`isSinkCall` は invoke の腕で一致しなかったとき **static callee の腕に落ちる**。
invoke では `StaticCallee()` が nil なので、invoke の腕が置いていった値
（**インターフェースの**パッケージとメソッド名）がそのまま残り、
最後のループが**パッケージ関数の sink** に一致しうる ——
`(pkg.I).Open` が `pkg.Open` の sink を満たしてしまう。
guff は invoke の腕で止める。5 つの表の範囲では到達不能で
（sink は全部 stdlib のパッケージで、同じパッケージの
インターフェースに同名のメソッドは無い）、その前提が崩れる表を足したときに
読み直すべき箇所としてモジュールのコメントに書いてある。

#### 性能

`lookup_named_type` は**オブジェクトアリーナ全体を歩く**（上流が
`prog.AllPackages()` のスコープを歩くのと同じ）。guard の答えを
`(実引数の型, 要求する型)` でキャッシュしただけでは、
**種類の違う writer ごとにパッケージ 1 つあたり 1 回**その歩きが走る。
アリーナはプログラム全体のものなので、`want` 単位でも memo する
（1 ルールにつき 1 回）。G705 の `want` は `net/http.ResponseWriter` の 1 つだけである。

#### 結果

**syncthing の gcl-only G705 は 3 → 1**、gosec 全体で
guff 97 / golangci 101 / both 95（P 97.9% / R 94.1%）。unexpected は 42 → 40。
**guff-only は 1 件も増えていない**（他の 15 ターゲットは完全に不変）。

そして **authelia が 16 → 15 に減った**。減ったのは finding ではなく
**guff の偽陽性**である:

```go
// internal/handlers/handler_oauth2_oidc_userinfo.go:202
//nolint:gosec // TODO: Run this line through taint analysis.
```

このディレクティブは G705 が無いあいだ「使われていない」ので
nolintlint が撃っていた。G705 が実装された瞬間に**ディレクティブが仕事を始め**、
偽陽性が 1 件消えた。**コメントが欠けている実装の名前をそのまま書いていた**。

#### 残り 1 件は G705 の表の問題ではない

`lib/api/api.go:1030` の

```go
func (*service) flushResponse(resp string, w http.ResponseWriter) {
	w.Write([]byte(resp + "\n"))
}
// 呼び出し側: s.flushResponse(`{"ok": "resetting folder `+folder+`"}`, w)
```

は `resp` が**素の string 引数**なので、taint は**呼び出しグラフ経由**
（`isParameterTainted`）でしか届かない。同じ形を 30 行に縮めた再現を作ると
**上流も黙る** —— つまり差が出るのはこの形そのものではなく、
syncthing の規模での**グラフの種**（`AllFunctions` / `RuntimeTypes` の到達）である。
続き 18 が `plainRec` / `boxedRec` で fixture 化したのと同じ半分で、
G702 / G703 / G706 / G710 と共有している。単独のタスクにすること。

**次にやること**

1. **seed の辺を「組むファイル」に合わせる** —— 続き 21 の 1。prometheus の 2 件。
2. **G115 の文言（`rune -> byte`）** —— 続き 18 の 3。
   gosec は `instr.X.Type().Underlying().(*types.Basic).Name()` を出す。
   go/types は `byte` / `rune` を **`aliases` 配列の別の `*Basic`**
   （kind は `Uint8` / `Int32`、name は `"byte"` / `"rune"`）として持ち、
   `range` 文字列や `byte(x)` 変換がその別名のほうを伝播させる。
   guff の `byte` / `rune` は `uint8` / `int32` と**同じ TypeId を指す TypeName**
   なので、「ソースが `byte` と書いた」情報が型に残らない。
   `Basic { kind, info, name }` という形は上流と同じなので、
   **kind が同じで name が違うアリーナ要素を 2 つ足す**のが素直な直し方になる。
   危ないのは basic を **TypeId で比較している**箇所で、そこを洗う必要がある。
   syncthing の `deviceid.go:191/209` が両側に並んでいる 2 行で、
   直せば gcl-only 2 と guff-only 2 が同時に消える。単独のタスクにすること。
3. **`call_name` の中央修正** —— 続き 23 の 1。
4. **複合リテラルの lowering** —— 続き 18 の 4。
5. **`assignable_to` の 2 段の順序** —— 続き 20 の 5。
### 2026-08-23（続き 25）— `byte` と `rune` は「同じ型の別名」ではなく「名前の違う別の型」

続き 24 の「次にやること」2。syncthing の `lib/protocol/deviceid.go:191/209` が
**両側に同時に並んでいた**唯一の差分:

```
+guff  G115: integer overflow conversion int32 -> uint8
+gcl   G115: integer overflow conversion rune -> byte
```

行も列も同じで、**綴りだけが違う**。

#### 原因 —— TypeName が 2 つ、Basic は 1 つ

gosec は `instr.X.Type().Underlying().(*types.Basic).Name()` を出す。
go/types は `byte` / `rune` を **`aliases` 配列の別の `*Basic`**
（kind は `Uint8` / `Int32`、name は `"byte"` / `"rune"`）として持っていて、
`identical` は **kind で比べる**ので代入も変換も何も変わらない。
別々の値が保っているのは**ソースがどちらの綴りを書いたか**だけであり、
診断に出るのはその名前のほうである。

guff は `byte` / `rune` を **`uint8` / `int32` と同じ TypeId を指す TypeName**
にしていた。`basic.rs` の `BYTE` / `RUNE` は
「`Uint8` / `Int32` と同じ数値の別名」というコメントつきでそう決めてあり、
**そこまでは正しい（kind は本当に同じ）**。取りこぼしていたのは
「kind が同じでも `*Basic` は別」という上流の一段である。

#### 直し方は 2 要素の追加で済んだ

`Basic { kind, info, name }` という形は最初から上流と同じで、
`identical` も既に `a.kind() == b.kind()` で比べていた。
なので**アリーナに 2 つ足して TypeName の向き先を変えるだけ**で、
型検査のロジックには 1 行も触っていない。

**ワークスペース全体のテストが通り、直したテストは 2 本だけ**だった:

1. `universe.rs::byte_and_rune_aliases_point_to_uint8_and_int32` ——
   古いモデルそのものを表明していたので、新しいモデル
   （**TypeId は別・kind は同じ・`identical` は真**）を表明するように書き直した。
2. `check_files.rs::conversions_to_type_literals_are_checked` ——
   `var z int = []byte(s)` のエラーが `[]uint8` を含むことを要求していた。
   **Go は `[]byte` と言う**:

   ```
   cannot use []byte(s) (value of type []byte) as int value in variable declaration
   ```

   つまりこのテストは**上流より不正確な綴りを固定していた**。
   狙っていた 1 件のほかに、この小さな不一致も一緒に消えた。

#### 綴りが残るのはパッケージ内だけ

export data は basic を **kind で**符号化するので、
パッケージ境界を越えた `byte` は `uint8` として復号される —— **Go でも同じ**。
syncthing の該当行は `luhn32` が同じ `lib/protocol` で
`(rune, error)` を返しているので、綴りが残る側に入っている。

#### 結果

`deviceid.go:191:27` / `209:27` が両方とも
`G115: integer overflow conversion rune -> byte` になり、上流と一致。
**gcl-only 2 件と guff-only 2 件が同時に消える** —— 同じ 2 行が
差分の両側に立っていたので、片側だけ直しても数は減らなかった。
golden 103/103、isolate 116/116 は不変（他のどのメッセージも綴りが動いていない）。
### 2026-08-23（続き 26）— 桁だけが違う欠陥は、桁を見るゲートを作らないと永久に見えない

続き 25 で syncthing の `deviceid.go` の 2 行を突き合わせていたら、
**すぐ上の行で godoclint が桁だけずれている**のが目に入った。

```
guff  lib/protocol/deviceid.go:22:1: godoc should start with symbol name ("ShortIDStringLength")
gcl   lib/protocol/deviceid.go:22:2: godoc should start with symbol name ("ShortIDStringLength")
```

#### 15 行に縮めると、左端かどうかで割れていた

```go
// WrongTop is misdocumented at column 1.
func TopLevel() {}          // 両方 3:1 —— 一致

const (
	A = 1
	// WrongInner is misdocumented, indented by a tab.
	Inner = 7               // guff 8:1 / gcl 8:2 —— ずれる
)
```

`godoclint` は**コメントを残したまま 2 度目のパース**をしていて、
2 つの `FileSet` は位置を独立に採番するので、
持ち帰れる共通の座標は (行, 桁) しかない。
guff は**行だけ**を持ち帰って `line_pos`（＝その行の先頭）を撃っていた。
左端の宣言では偶然それが正解になるので、
**`const (` や `var (` の中でインデントされた doc コメントだけがずれる。**

#### どのゲートにも写らない形だった

- **isolate の fixture は トップレベルの func が 1 つ**。両側とも桁 1 で一致する。
- **isolate と OSS/hunt の比較キーは `path:line:linter:message`** ——
  §1 が最初に数えた「column を一切比較していない」がそのまま効いている。

つまり**桁を見る tier が無い check は、桁がずれていても永久に緑**である。
`compat/golden/cases/forcetypeassert/config.yml` が同じことを書いている ——
あの case はまさにこの理由で作られている。

#### hunt は 1 ターゲットも動かない —— それがこの話の証明である

修正後の hunt は **16 ターゲット全部が完全に不変**（failures=0 / health=0）。
出力は実際に変わっているのに、**hunt のキーには桁が無いので写らない**。
「桁を見るゲートを作らないと永久に見えない」は比喩ではなく、
この 0 という数字がそれである。

#### 直し方とゲート

`reparsed_pos` を 1 つ置いて、再パース側の `Position` から
**行と桁の両方**を持ち帰るようにした（`column` は 1 始まりのバイト桁、
`line_start` はバイトオフセットなので足し算は近似ではなく厳密）。
パッケージ doc の側も同じ経路に寄せた。

- **`compat/golden/cases/godoclint` を新設**（golden は **103 → 104 case**）。
  `path:line:col:linter:severity:text` を正規化なしで比較する唯一の tier で、
  fixture に**左端の宣言と、`const (` / `var (` の中の宣言の両方**を入れてある。
  上流の答えは `bad/bad.go:17:2` と `28:2`。
- **`godoclint_reports_the_column_the_doc_comment_starts_at`** ——
  メッセージではなく `line:column` を表明する。
  `support::run_analyzer_positions` を足した（位置が欠陥である check 用）。
  **両方向で確認**: 桁を捨てて行頭に戻すと `17:1` になって落ちる。

#### 棚卸ししたら、同じ形が隣に 1 つ、別の欠陥が 1 つ出た

**golden に case がある linter は 116 中 21 だけ。残る 95 は桁が構造的に自由である。**

```
comm -23 <(ls compat/isolate/fixtures/ | sort) \
         <(ls compat/golden/cases/ | sed 's/-.*//' | sort -u) | wc -l   # 95
```

優先順位は「位置を自前で計算しているか」で付く。
**行番号から復元している**（＝今回と同じ形の）check は
`grep -rln "line_pos(" crates/*/src/` で 6 つ:
`dupl` / `dupword` / `godoclint` / `godot` / `godox` / `gomoddirectives`。

その 3 つを 22 行の fixture で上流と突き合わせたら、**2 件出た**:

1. **`dupword` は同じ桁の欠陥を持っていた（この回で一緒に直した）。**
   `var (` の中のコメントで guff `13:1` / 上流 `13:2`。
   `dupword` 自身の fixture のコメントが func の中でインデントされているので、
   **最初からずっと外していた** —— 誰も桁を見ていなかっただけである。
   文字列リテラル側（`check_string_lit`）は `lit.value_pos` を
   解析側の `FileSet` から直接読むので**一度も影響を受けていない**。
   golden case はその 2 つを並べて pin してある（`4:2` のコメントと
   `5:10` のリテラル）。
2. **`godot` は `var (` / `const (` の中の doc コメントを見ていない。**
   22 行の fixture で上流 4 件に対し guff 2 件（8 行目と 13 行目を落とす）。
   これは桁ではなく **recall** で、`util.rs` と `godot.rs` に
   `DEFERRED: getBlockComments` として**既に書いてある**既知の欠落である。
   書いてあることと、いくらの値段が付いているかは別なので、ここに値段を記録する。

#### `line_pos` 一族は 6 つとも測った —— 欠陥は 2 つ

| check | 結果 |
|---|---|
| `godoclint` | **欠陥**。この回で修正 |
| `dupword` | **欠陥**（コメント側のみ）。この回で修正 |
| `godox` | 正しい。`line_start + start_col` を足していて、しかも**上流の `+1` の癖**（golangci の wrapper が `i.Pos.Column + 1` を出す）までコメントに書いてある |
| `dupl` | 正しい。そもそも桁を出さない（`a.go:3:`） |
| `gomoddirectives` | 正しい。go.mod のディレクティブは本当に桁 1 |

**「行から復元している」は疑いの根拠であって、欠陥の証拠ではない** ——
6 つのうち 4 つは正しく、うち 1 つは正しさの理由をコメントに書いてあった。
測るまでは分からない、というのがこの表の中身である。

**次にやること**

1. **`godot` の `getBlockComments`** —— 上の実測つき（22 行で 4 対 2）。recall 側。
2. **golden case の拡充** —— 残り 93。位置を自前で計算している側から。
   `line_pos` 一族は尽きたので、次の切り口は
   「ノードではなくトークンを撃っている」「オフセット演算をしている」側
   （`grep -rlnE "pos \+ [0-9]|\.0 as u32 \+ "` で 14 ファイル）。
2. **seed の辺を「組むファイル」に合わせる** —— 続き 21 の 1。prometheus の 2 件。
3. **`call_name` の中央修正** —— 続き 23 の 1。
4. **複合リテラルの lowering** —— 続き 18 の 4。
5. **`assignable_to` の 2 段の順序** —— 続き 20 の 5。
6. **G705 の残り 1 件（呼び出しグラフ）** —— 続き 24。
### 2026-08-23（続き 27）— `DEFERRED` と書いてあることと、値段が付いていることは別である

続き 26 の「次にやること」1。`godot` の `getBlockComments` 欠落は
`util.rs` と `godot.rs` の**両方にコメントで書いてあった**。
書いていなかったのは、それがいくらの findings に相当するかである。

22 行の fixture で **上流 4 件 / guff 2 件**。
`var (` と `const (` の中の doc コメントを丸ごと見ていなかった。

#### 上流の `declarations` は 2 つの和である

```go
case DeclScope:
    comments = append(pf.getBlockComments(exclude), decl...)
```

guff は `decl`（＝ `getDeclarationComments`）の側しか持っていなかった。
`tetafro/godot` はローカルに checkout があるので、`getBlockComments` を
そのまま移植した。効いている条件は 3 つで、**どれも形から推測できない**:

1. **`Lparen` があるものだけ** —— `const C = 3` は 1 行なので「中」が無い。
2. **走るのは `file.Comments`** であって spec の `Doc` ではない。
   だから**どの spec のものでもない浮いたコメント**も入る。
3. **桁がちょうど 2 のものだけ**。上流のコメントが理由を書いている ——
   ブロック自体がトップレベルなので、その**直下**だけが対象になる。
   もう 1 段深いコメント（桁 3）は**意図的に**捨てられる。

3 番目は特に、読まずに書いたら確実に落とす。

#### ゲート

- **`compat/golden/cases/godot` を新設**（golden は **104 → 105 case**。
  続き 26 の 2 つと合わせて **103 → 106**）。
  fixture は 5 件を pin する: トップレベルの decl doc 3 つと、
  ブロックの中の 2 つ。そして**黙るべき 3 つ**を同じファイルに置いてある ——
  桁 3 のコメント（複合リテラルの中）、func 本体の中のコメント、
  そして `Lparen` の無い 1 行 `const`。
- **両方向で確認**: `block_comments` を外すと golden が
  `bad/bad.go:15` と `:20` を落として fail する。
- `godot_checks_comments_inside_top_level_blocks` ——
  godot のメッセージは全部同じ文字列なので、
  **単体テストで言えるのは件数だけ**（3 → 5）。
  どの 5 件かは golden が行と桁で pin する。そう書いてある。

#### この 2 回で分かったこと

続き 26 と 27 は同じ場所（`crates/guff-comment`）の別の欠陥で、
**片方は誰も知らず、もう片方は 2 箇所にコメントで書いてあった**。
見つかり方は同じ —— **上流と 1 件ずつ突き合わせる fixture を作った**だけである。
`DEFERRED` は「後で見る」の印であって「今いくら損しているか」ではないので、
棚卸しするなら**値段を付けて回る**のが先になる。

#### 追記 —— この直後に `--all-linters` を回したら、同じ linter でもう 2 件出た

Phase 2 の tier（`./compat/run.sh --oss --tier pr --all-linters`）は
**ハーネスだけ完成していて差分が一度も消化されていなかった**。回した。

まず tier 自身についての結果が出た: **5 ターゲット中 4 つで、
golangci-lint が 4 回走って一度も同じ finding 集合を返さなかった。**
全 linter を有効にすると上流の出力が安定しない —— syncthing の
`gcl 654 / 656` の揺れと同じもので、この tier が gate になっていない理由でもある。

そのうえで cobra の差分を linter 別に並べると、**guff-only と gcl-only が
同数**の組がいくつも出る（`godot` 11/11、`wrapcheck` 7/7、`usetesting` 5/5）。
**同数は「実装が無い」ではなく「位置か文言がずれている」の署名**である。

`godot` の 11/11 を開いたら、位置が 1〜3 行ずれていた。原因は 2 つ:

1. **複数行コメントで報告する行が違う。** 上流の `checkPeriod` は
   **最後の空でない行**を報告する（`pos.line + c.start.Line - 1`）。
   guff はコメントの開始行を報告していた。
2. **空の `//` 行が本文から丸ごと消えていた。**
   `comment_check_text` が `stripped.lines()` で回していて、
   `"".lines()` は**要素を 1 つも返さない**。だから `//` だけの行が落ち、
   それ以降の行番号が 1 つずつ手前にずれる。1 を直しても
   `//` を含むコメントだけ 1 行ずれたままだったのはこれである。

上流の**桁**は移植していない。`checkPeriod` は
「最後の行の末尾の 1 つ先」を計算しているが、
**golangci-lint はそれを捨てて桁 1 を出す**（cobra の JSON で確認）。
同じことを `godox` のコメントが既に書いている。

**cobra の godot は 11 件ずれ → 27 対 27 で完全一致**になった。

#### 自分で足したゲートが、自分の欠陥を通した

続き 27 の本文で足した `compat/golden/cases/godot` の fixture は
**単一行コメントしか持っていなかった**ので、上の 2 件が生きたまま緑だった。
**思いついた形しか入っていない fixture は、その形以外については無いのと同じ**である。
fixture に 2 行コメント・空行を含むコメント・末尾が空行のコメントを足した
（golden は 5 → 8 件）。
### 2026-08-23（続き 28）— 「guff-only と gcl-only が同数」は偶然ではなく署名である

続き 27 の追記で `--all-linters` を回したとき、cobra の差分を linter 別に並べると
**guff-only と gcl-only が同数**の組がいくつも出た。
`godot` 11/11、`wrapcheck` 7/7、`usetesting` 5/5、`nonamedreturns` 2/2、`nlreturn` 2/2。

**同数は「実装が無い」ではなく「同じ場所を違う綴り・違う位置で報告している」の署名**である
—— 1 件の食い違いが差分の両側に 1 件ずつ立つので、数が揃う。
`godot` を開いて 2 件出た（続き 27 の追記）。残りも開いた。

#### `wrapcheck` —— レシーバが消えていた（#101 と同じ根）

```
guff  sig: func os/exec.StdinPipe() (io.WriteCloser, error)
gcl   sig: func (*os/exec.Cmd).StdinPipe() (io.WriteCloser, error)
```

上流は `types.Func.String()` を出す。go/types の `writeFuncName` は
**メソッドを `(RecvType).Name` と書く**（レシーバがインターフェースなら
`(interface).Name`）。guff は `obj.pkg()` からパッケージ修飾を組み立てていたので、
メソッドが `os/exec.StdinPipe` になっていた —— **Go が決して出さない綴り**である。

これは errchkjson（#101、続き 23）と**同じ根**で、
**`code::type_func_name` がまさにその関数**であり、既にあった。
続き 23 が数えた「別々に作り直していた 5 か所」に **wrapcheck が 6 番目として加わる**
——「必要としたのに、既にある 1 つを使わなかった」側で 3 例目。
影響はメッセージだけではない: 利用者が `ignoreSigs` に書くパターンも
この文字列と突き合わされるので、**設定が効かない**。

位置も違った。上流は `call.Pos()`（CallExpr では `Fun.Pos()`）を報告し、
guff は `call.lparen` を報告していた。**セレクタ呼び出しのときだけ**ずれるので、
`return f()` は一致して `return x.M()` はずれる。

**cobra の wrapcheck: 7 件の綴り違い + 5 件の桁違い → 31 対 31 で完全一致。**

#### `nonamedreturns` —— 型が「型ではない文字列」だった

```
guff  named return "f" with type "func(...)" found
gcl   named return "f" with type "func(*Command) error" found
```

上流はメッセージに `go/types.ExprString` の出力を入れる。
guff の `nonamedreturns.rs` には**自前の**「Approximate `go/types.ExprString`」があり、
`FuncType` の腕が `"func(...)"` という**誰も書いていない文字列**を返していた。
ついでにチャネルの向き（`<-chan` / `chan<-`）も落としていた。
`writeSigExpr` / `writeFieldList` を移植した。

位置も違った。上流は **`func` キーワード**（`(*ast.FuncDecl).Pos()`、
リテラルも同じ）を報告し、guff は名前付き戻り値の識別子を報告していた。
**左端の宣言では偶然一致する**ので、`var x = func() (f …)` の中でだけ割れる。

**cobra の nonamedreturns: 10 対 10 で完全一致。**

#### `expr_string` が 3 つある

`nonamedreturns` の「近似 ExprString」は 1 つではない。
`guff-revive/src/util.rs` と `guff-error/src/util.rs` にも別の `expr_string` があり、
**どれも上流の `types.ExprString` の一部しか持っていない**。
続き 23 の `call_name` と同じ形（1 つの上流関数を、複数の移植が別々に近似する）で、
これで 2 例目である。中央に 1 つ置く価値がある。

#### fixture は 3 つとも「届かない」形だった

| linter | isolate fixture | 届かなかった理由 |
|---|---|---|
| `wrapcheck` | 8 行・1 件 | **メソッド呼び出しが 1 つも無い** |
| `nonamedreturns` | 6 行・1 件 | 戻り値が `int` だけ |
| `godot`（続き 27） | —— | 単一行コメントだけ |

3 つとも「その linter が撃つこと」は確かめていて、
**「何を撃つか」は一度も確かめていない**。§1 が数えた 72 件の中身がこれである。
それぞれ 3 件 / 8 件に増やし、桁を見る golden case を足した
（この 2 つで golden は **103 → 105 case**。続き 26 の 2 つと続き 27 の 1 つを
合わせると 108 になるが、それらは別の枝にある）。

#### `usetesting` —— 5/5 も同じ署名だった

```
guff  os.CreateTemp("", ...) could be replaced by t.TempDir() in TestX
gcl   os.CreateTemp("", ...) could be replaced by os.CreateTemp(t.TempDir(), ...) in TestX
```

`os.CreateTemp` は**このリンタで唯一「呼び出しを残す」提案**である ——
一時ファイルは作るので、変えるのは第 1 引数だけ。
他の 6 つは `pkg.Name() could be replaced by t.Name()` という同じ形なので、
guff はそれを流用していた。

**9 つの腕を全部測った**（`CreateTemp` / `MkdirTemp` / `TempDir` / `Setenv` /
`Chdir` / `context.Background` / `context.TODO`、`t` と `b` の両方）。
**違っていたのは `CreateTemp` の 1 つだけ**で、残り 8 つは文言も桁も一致していた ——
差分に出た 1 つだけを直して終わりにしない、というのはこのためである。

isolate の fixture は `MkdirTemp` と `TempDir` を持っていて `CreateTemp` を持っていなかった。
**合っている腕だけが入っていた。** これで 4 つ目である。

#### ついでに: `--all-linters` は gated tier の記録を上書きする

`./compat/run.sh --oss --tier pr --all-linters` は
**`compat/results/RESULTS.md` に書く** —— 通常の OSS tier（CI ゲート）と同じファイルである。
つまり発見用の tier を 1 回回すと、**ゲートされている tier の記録が黙って差し替わる**。
`hunt.sh` が `health-hunt.json` を分けている理由（「hunt の refresh が
OSS の gated な数字を動かせる形にはしない」）とまったく同じ話で、
results 側は分けられていない。この回は手で戻した。
tier ごとにファイルを分けるべきである。

#### `local` の gocritic 72/72 —— 署名は当たったが、直さなかった

`--all-linters` で summary が出たもう 1 つのターゲット `local` は
**gocritic が 72/72**。開くと 1 つの欠陥が 72 回出ているだけだった:

```
guff  assignOp: replace `sum = sum + k * 2` with `sum += k * 2`
gcl   assignOp: replace `sum = sum + k*2`   with `sum += k*2`
```

上流の `assignOp` は **ruleguard のルール**（`checkers/rules/rules.go`）で、

```go
m.Match(`$x = $x + $y`).Where(m["x"].Pure).Report("replace `$$` with `$x += $y`")
```

`$$` / `$x` / `$y` は**書かれたとおりのソース片**に置換される。
だから `k*2` と書いてあれば `k*2`、`(k + n)` と書いてあれば `(k + n)` が出る。

guff は AST から**印字し直して**いる。`node_text`（＝ guff の go/printer 移植、
`walkBinary` / `cutoff` まで入っている）に替えても直らない ——
**部分式を単独で印字すると `k * 2` が正しい**からである。
go/printer は深さ 1 で優先順位が 1 種類しか無いとき空白を入れ、
混在した式の中でだけ落とす。`sum + k*2` 全体を印字すれば `k*2` になるが、
`$y` は単独で置換される。

**直していない。** 正しい直し方は
「ruleguard 系のチェックはソースを切り出して置換する」で、
`expr_text` の呼び出し箇所は **118 か所**ある。単独のタスクにすること。
値段は測ってある: **合成ターゲット `local` で 72 件、cobra では 0 件**
（cobra の gocritic 差分は 0/0）。実リポでの実害は今のところ確認できていない。
`gocritic.rs` の該当箇所にこの経緯をコメントで残した。

**「署名が当たる」と「直す価値がある」は別**である。72 という数は大きく見えるが、
中身は 1 つの欠陥 × 合成ファイル 72 個だった。

---

### 2026-08-23（続き 29）— seed が組むファイルと、seed を並べる辺が、別の写しから来ていた

続き 21 の「次にやること」1。prometheus `./...` の ill-typed 2 件
（`promql_test` と `cmd/promtool`、どちらも
`cannot use invalid type value as *promql.Engine value`）。

続き 21 の診断は当たっていた —— **seed は依存を production ファイルで組むのに、
並べる辺を test variant から採っていた**。外していたのは直す場所で、
「パッケージロード側の設計変更」ではなく `import_path_dep_graph` の
**写しの選び方 1 つ**だった。

#### `go list -test` の括弧 id は 3 種類あり、ファイルも辺も違う

| id | ファイル | 辺 |
|---|---|---|
| `P` | production | production |
| `P [Q.test]` | production（Q のテストバイナリ向けに再コンパイルしただけ） | production |
| `P [P.test]` | production **＋** P の同一パッケージ `_test.go` | production ＋ **テストの import** |

`filter_duplicate_packages` は `P [P.test]` が居ると plain `P` を落とす。
`./...` のリポではテストを持つパッケージがほとんどなので、
**ほぼ全部の path が「括弧つきの写しのどれか」を選ばされる**。
そこで `P [P.test]` を採ると、テストの import が辺に混ざる ——
そしてテストの import は**リポの中へ戻ってくる**。

prometheus:

- `tsdb` の同一パッケージテストが `util/teststorage` を import する
- `util/teststorage` の同一パッケージテストが `tsdb` を import する

production 側はどちらも相手を import していないので Go は通る
（それぞれのテストバイナリの中では相手が production の写しになる）。
両方の key が test variant から辺を採って初めて、**Go に存在しない循環**ができる。

#### 循環が何を壊すか

`dep_load_order` の `visiting` ガードが辺を 1 本落として walk を終える。
`order` はもう位相順ではなく、`height` のパス 2 は
**まだ確定していない consumer の height を読む**。prometheus `./...` で
`wave(dep) < wave(consumer)` を破る辺が **39 本**（consumer は 38 本が `tsdb`、
残り 1 本が `promql/promqltest`）。

`GUFF_DEBUG_SEED_ERRORS=1` が 2 行で言う:

```
guff:     seed dep github.com/prometheus/prometheus/tsdb — 73 error(s), first: undefined: index
guff:     seed dep github.com/prometheus/prometheus/promql/promqltest — 19 error(s), first: undefined: promql
```

どちらも「自分の依存がまだ merge されていない」である。
seed の依存の診断は報告されない（利用者のコードではない）ので、
**見えるのは 2 つ隣で ill-typed になったパッケージだけ** ——
`promql_test` と `cmd/promtool` の
`cannot use invalid type value as *promql.Engine value` である。

#### 直し方 —— 辺は「seed が組むファイルを持つ写し」から採る

`seed_variant_rank` が path ごとに 1 つの写しを選び、
`seed_package_for`（ファイル）と `import_path_dep_graph`（辺）が**同じ関数**を読む。
順位は「ファイルが空でない」→ 次の種別:

| path | 1 位 | 2 位 | 3 位 |
|---|---|---|---|
| 外部テストパッケージを持つ（＝ seed が `_test.go` も組む） | `P [P.test]` | `P` | その他 |
| それ以外（＝ seed は production だけ組む） | `P` | `P [Q.test]` | `P [P.test]` |

**`P [Q.test]` は production の写しそのもの**なので、production の辺を持っている。
「plain `P` はもう手元に無い」の答えは、
**別の名前で同じものが既に手元にあった**だった。

残るのは「`P [P.test]` しか無い」形 —— 誰のテストバイナリも P を再コンパイル
していない path。ここだけは写しが無いので、`filter_duplicate_packages` が
plain を落とす**直前**に plain の `deps` を survivor へ写す
（`carry_production_deps` / `Package::production_deps`）。
dapr の `pkg/runtime/pubsub` と rclone の `backend/local` がこの形で、
それぞれ **9 本 / 19 本**の破れた辺を出していた（どちらも ill-typed には至っていない）。

#### 選択が HashMap の反復順に乗っていた

旧コードの非 authoritative 側は `or_insert` で、
**FxHashMap の反復順で先に来た写しが勝つ**。同じロードでは再現するが、
**パッケージが 1 つ増減しただけで別の写しが勝ちうる** ——
つまり「直したつもりが隣のリポで再発する」側の欠陥である。
続き 21 が同じ prometheus で破れた辺を 16 本と数え、今日は 39 本だった ——
間に続き 21 の augment 修正が入って id 集合が動いている以上、
**同じリポの同じパターンでも数が変わる**のは驚くことではない
（どちらの数が正しいという話ではなく、どちらも「その日の反復順」の値である）。
新しい選択は (ファイルの有無, 種別, id) の**全順序**なので、反復順に依存しない。

`package_for_import_path` の `values().find()` も同じ穴だった ——
**ファイルの出所が反復順で決まっていた**。今は `seed_variant_for` に寄せてあり、
`files_and_edges_are_read_off_the_same_variant` がその一致を撃つ。

#### ゲート —— 「循環は起きた」を常時出す

seed の dep graph は、この修正のもとでは**構造的に非巡回**である:
同一パッケージの `_test.go` も `package p` なので、Go の import cycle 禁止が
そのまま効く（`p` を import するものを `p` のテストは import できない）。
外部テストパッケージ `p_test` は別の key なので巻き込まない。
つまり **back-edge が 1 本でも出たら guff のバグ**であり、リンタ対象の性質ではない。

`dep_load_order` が落とした back-edge を返し、`guff: seed dep cycle A -> B` を
**常時 stderr に出す**（先頭 3 本＋残り件数）。`compat/health.py` はこれを
**panic と同じ扱い**にした —— baseline に載せる欄は無く、無条件で落ちる。
ill-typed のように「baseline より増えたら」ではないのは、
**ill-typed になるのは 2 つ隣のパッケージで、順序が偶然通っている回は何も起きない**からで、
原因の側で撃たないと「今日は緑」が保証にならない。

コーパス 27 リポ（hunt 16 + repos 11）で **back-edge 0 本**。

#### 測定

| | base | 修正後 |
|---|---|---|
| prometheus 破れた辺 | 39 | **0** |
| prometheus ill-typed | 2 | **0** |
| prometheus seed 深さ | 56 | 52 |
| dapr / rclone 破れた辺 | 9 / 19 | **0 / 0** |
| grafana ill-typed | 9 | **0** |
| コーパス 27 リポの back-edge | —— | **0** |

**両方の health baseline が空になった。**

- `health-hunt.json`: prometheus 2 → 0。**hunt 16 ターゲット全部が 0**。
- `health.json`: grafana 11 → 0（実測は 9）、caddy 1 / consul 2 / gin 1 → 0。
  **OSS 10 ターゲット全部が 0。**

行が 1 つも無い＝全部が厳密に 0 でゲートされる。
ただし **caddy / consul / gin の 3 行は本修正とは関係なく、base バイナリでも 0 だった** ——
以前のセッションで直ったまま行が残っていた**古い許容**である。
grafana の 9 だけが本当に隠れていて、そこから下の偽陽性が出た（次節）。

#### 差分は 2 ターゲットで減り、残り 14 は 1 バイトも動いていない

`hunt-20260823T035454Z`（前回の全体走査）と突き合わせた:

| target | before | after |
|---|---|---|
| authelia | guff 16 / gcl 0、**ill-typed 1** | guff 15 / gcl 0、**0** |
| syncthing | 638 / 654 / both 625（unexpected 42） | **647 / 654 / both 636（29）** |
| 他 14 | —— | 完全一致 |

authelia の ill-typed は `internal/storage_test`（30 errors）——
**外部テストパッケージ**で、まさにこのクラスである。
syncthing は ill-typed が 0 のまま **both が 11 増えた**。

**syncthing の guff 側の件数は、コードを変えていない回どうしでも
638 / 640 / 645 と揺れていた**（`hunt-20260822T112822Z` 以降の 11 回）。
今回の 647 は記録上の最良で、unexpected 29 も最小である。
揺れの説明は 1 つに絞れていないが、**写しの選択が反復順に乗っていたこと**は
候補として矛盾しない —— C-7 の投機 seed（`peeked_graph_shape` の推測グラフ）と
本番のロードでは id 集合が違い、どちらが間に合うかは**タイミング**で決まるので、
2 つの経路が別の写しを選びうる。両方に `carry_production_deps` を通してある。
**断定はできない**（この揺れを直接測ってはいない）が、
次に syncthing の件数が揺れたらここを最初に疑うこと。

#### 直したら 1 件出てきた —— grafana の ill-typed 9 件が黙らせていた偽陽性

**OSS nightly の grafana は `0 vs 0` で通っていた。** 実際には
`pkg/storage/unified/{sql,search,resource}` など **9 パッケージが ill-typed** で、
そこは誰も解析していなかっただけである（`compat/baselines/health.json` の
grafana 行は 11 を許していた）。修正後は **0** になり、
そこで 1 件の **guff 側の偽陽性**が出てきた:

```
pkg/storage/unified/resource/storage_backend.go:247:3:
  ineffectual assignment to searchLookback
```

読むと guff のほうが「正しい」—— 245 で作った local は 247 で上書きされ、
その後どこからも読まれない（284 の構造体リテラルが読むのは `opts.SearchLookback`）。
しかし上流は撃たない。理由は上流の walk にある:

```go
// gordonklaus/ineffassign: CompositeLit も KeyValueExpr も case が無い
case *ast.Ident:
    bld.use(n)
```

`case` が無いので既定の walk が `KeyValueExpr` の**キーにも入り**、
`bld.use(id)` は **`id.Obj` ＝ go/parser のスコープ解決**で引く。
go/parser は `T{v: x}`（フィールド名）と `map[K]V{v: x}`（`v` の読み出し）を
**区別できない**ので、同名の local が見えていればキーをそれに束ねる
（go.dev/issue/45160 と同じ曖昧さ）。だから上流にとって 247 の代入は「使われている」。

guff は go/types の `uses` で引いていた —— そちらはキーが**フィールド**だと知っている。
**正しいほうを見たせいで、上流が撃たないものを撃っていた。**

guff のパーサは上流と同じ解決をしている（`parser_resolver::walk_composite_lit` が
`resolve(id, false)`）ので、キーだけ `Ident.obj` で引き直す
（`decl_ident_id` が Object の宣言側 Ident の node id を返し、`Info.defs` を通して
`ObjectId` に戻す）。最小再現:

```go
func A(x int) S { v := x; if v < 0 { v = 42 }; return S{v: x} }    // 両者とも撃たない
func B(x int) S { v := x; if v < 0 { v = 42 }; return S{oth: x} }  // 両者とも撃つ
```

ゲートは 3 つ: Rust テスト 2 本（両向き）、**golden case `ineffassign`**
（この linter には golden が 1 つも無く、**桁が一度も検証されていなかった** ——
1 メッセージ 1 位置の linter なので、桁は読み手が 2 件を見分ける唯一の手掛かりである。
108 → **109 case**）、isolate fixture を 8 行 1 件から 2 件に広げた。

**「ill-typed を直すと差分が出る」の実例**である（続き 6 / 続き 16 と同じ形）。
`0 vs 0` は「一致している」ではなく「**どちらも何も言っていない**」でもありうる。

#### テスト

- `dedup.rs`: `a_for_test_dep_copy_outranks_the_test_augmented_one`（prometheus の
  形を縮めた 4 パッケージ）、`carried_production_deps_stand_in_for_the_dropped_plain_package`、
  `an_augmented_path_keeps_its_test_variant_edges_after_a_carry`（続き 21 の回帰止め）、
  `nothing_is_carried_when_the_plain_package_survives`、
  `files_and_edges_are_read_off_the_same_variant`。
  **旧実装をそのまま戻して 2 本が落ちることを確認した**（片方向だけでは、
  新しい実装が新しいテストに合っているという以上のことを言わない）。
- `typecheck.rs`: `dep_load_order_reports_the_back_edge_it_had_to_drop` /
  `a_diamond_is_not_a_cycle`（共有と循環の区別）/
  `dep_load_order_is_leaves_first_and_reports_no_cycle`。
- `compat/tests/test_health.py`: seed cycle は headroom があっても落ちること、
  **ill-typed が 0 の target でも落ちること**。
- `guff-ineffassign`: `ineffassign_treats_a_field_key_spelled_like_a_local_as_a_use` と
  `ineffassign_still_flags_a_dead_store_when_the_key_names_another_field`。
  **後者が無いと「複合リテラルの近くの代入は撃たない」に退化しても誰も気付かない。**

**次にやること**

1. **golden tier のカバレッジ** —— 116 linter 中 94 に golden case が無く、
   column と severity が構造的に未検証。#104 / #105 / #106 と、今回の ineffassign も
   この穴から出た。
2. **`expr_string` を 1 つにする** —— 続き 28 の 2 例目。`guff-revive/src/util.rs` /
   `guff-error/src/util.rs` / `nonamedreturns.rs` に別々の近似がある。
3. **`compat/results/` を tier ごとに分ける** —— 続き 28 の指摘。
   `--tier pr` を回すと nightly の行が黙って消える（このセッションでも踏んだ）。
4. **gocritic の ruleguard `$`** —— 続き 28。`expr_text` 118 箇所、単独タスク。

---

### 2026-08-24（続き 30）— 116 linter 中 84 は「桁を見るゲート」を一度も通っていなかった

golden tier のカバレッジ（続き 29 の「次にやること」1）。

#### 数えると、無いのは 84 だった

golden の case は 109 あったが、**`linters.enable` に現れる linter は 32** ——
残り 84 は **golden case が 1 つも無い**。他の tier は
`compat/normalize.py` を通した `path:line:linter:message` で突き合わせるので、
**column も severity も、normalize が消す文言差も、構造的に見えない**（§1）。
つまりこの 84 について、**桁と severity は誰も検証していなかった**。

isolate fixture は 116 全部にあるので、**それを `sources.txt` で golden に載せるだけ**で
84 が塞がる。fixture は両 tier が同じファイルを読むので、
片方のために広げた形はもう片方でも測られる。

#### 1 周目で 13 件出た。桁だけの欠陥である

75 case（stdlib だけで足りるもの）を足して guff を当てると、**13 case が不一致**。
全部「1 対 1 で match=0」＝ **同じ finding を違う桁で報告していた**。

**クラス A — 関数宣言の位置（5 linter、1 つの原因）**

`cyclop` / `gocognit` / `gocyclo` / `ireturn` / `paralleltest`。
上流はどれも **`FuncDecl.Pos()`** を報告する（go/ast ではこれは
`d.Type.Pos()` ＝ **`func` キーワード**）。guff は 5 つとも
`fd.name` を報告していた ―― `func Bad()` なら 1 桁目ではなく 6 桁目。
続き 28 の nonamedreturns とまったく同じ形で、**5 か所が別々に同じ間違いをしていた**。
後から `maintidx` が 6 つ目として加わった（下記）。

**クラス B — 式のどこを指すか（各 linter 固有。上流を読むしかない）**

| linter | 上流 | guff が指していたもの |
|---|---|---|
| `durationcheck` | `expr.Pos()`（BinaryExpr ＝ 左オペランド） | 演算子 |
| `err113` | `ce.Pos()`（CallExpr ＝ `Fun.Pos()`） | `(`（#106 の wrapcheck と同じ） |
| `exhaustruct` | `lit.Pos()`（CompositeLit ＝ 型があれば `Type.Pos()`） | `{` |
| `fatcontext` | `assignStmt.Pos()`（＝ 左辺の先頭） | `=` |
| `varnamelen` | `variable.assign.Pos()`（同上） | `:=` |
| `gosmopolitan` | `*ast.Ident` を歩いて `n.Pos()` ＝ **`Local`** | `time`（セレクタの根） |
| `rowserrcheck` / `sqlclosecheck` | **SSA 命令**の `Pos()` | 呼び出し式の先頭 |
| `nestif` | **golangci のラッパが桁を捨てる** | `if` |

2 つだけ説明を足す。

**`rowserrcheck` / `sqlclosecheck`** は `pass.Reportf(instr.Pos(), …)` ―― AST ではなく
**go/ssa の命令**の位置である。go/ssa は呼び出しの位置を**左括弧**にする
（`builder.go` の `c.pos = e.Lparen`、`(*Call).Pos()` がそれを返す）。
AST から命令を組み直すなら、**この規約ごと組み直す**必要がある。
両ファイルに同じ `assign_report_pos` の写しがあり、両方が同じ間違いをしていた。

**`nestif`** は上流と違う場所を報告するのが正解、という珍しい形。
nestif 自身は `fset.Position(stmt.Pos())`（＝ `if`）を記録するが、
**golangci のラッパが `f.LineStart(issue.Pos.Line)` に差し替える** ――
行頭、つまり**桁 1**。nestif の finding はすべてインデントの中にあるので、
`if` を報告すると**この linter が出す全 finding が 1 タブ分ずれる**。

**クラス C — 文言（`paralleltest`）**

上流の 5 つのメッセージのうち **4 つが `\n` で終わる**。
正規化する tier は末尾の空白を落とすので、**golden の key 以外に見えるものが無い**。
`t.Cleanup` の 1 つだけは `\n` が無く、5 つ全部に付けるのは全部から落とすのと同じくらい違う。

#### 依存が要る 9 件も、stub モジュールで塞いだ

`ginkgolinter` / `spancheck` / `promlinter` / `zerologlint` / `exptostd` /
`arangolint` / `clickhouselint` / `gomodguard` / `gomodguard_v2` は
外部モジュールを import する。golden は smoke ジョブ（速い側）で回るので、
**`replace` で差し込む入れ子モジュール**にした ―― `cases/protogetter` と
`cases/gosec` が既に使っている形で、`./...` は入れ子モジュールを飛ばすから
どちらのツールも stub を lint しないし、ネットワークも要らない。
6 つは `crates/*/tests/testdata/<linter>/stub/` の既存 stub をそのまま使える。

**`gomodguard` は go.mod を読む linter**なので、`require` 行さえあれば
`replace` されていても撃つ（実測）。

#### `ginkgolinter` が 0 件を書いた ―― そして「0 件の golden」という穴が見つかった

`ginkgolinter` の golden は最初 **0 key** で書かれた。**これが通ってしまうのが問題**である:
0 件の golden は「上流が何も報告しなかった」と
「**モジュールが壊れていて実行が空振りした**」の区別が付かない。

原因は上流の `GetGomegaHandler`：
**`github.com/onsi/gomega/types` パッケージが到達可能で、そこに
`Assertion` / `AsyncAssertion` / `GomegaMatcher` の 3 インターフェースが居る**
ことを最初に確かめ、無ければ **handler ごと nil を返して何もしない**。
共有 stub には `types` サブパッケージが無かった。
`types` を足し、`OmegaMatcher` / `Assertion` / `AsyncAssertion` を本物と同じく
そのエイリアスにしたら **6 件**（isolate の 6/6 と一致）。Rust 側のテストも通ったままである。

そこで **`compat/tests/test_golden_coverage.py` に「空の golden は落とす」を入れた**。
意図的に 0 件にしたい case は config.yml に `golden-may-be-empty` と書く。

#### 空の golden を弾いたら、7 件が引っかかった ―― §1 が数えた「空振り合格」である

`iface` / `maintidx` / `mirror` / `musttag` / `sloglint` / `usestdlibvars` /
`varnamelen`。**§1 が 2026-08-07 に「`both == 0` の空振り合格 9 linter」と
書いたもののうち 7 つ**が、そのまま残っていた。fixture がその linter の
発火条件を満たしていない ―― 「撃つこと」すら確かめていない側である。

上流を読んで 7 つとも撃つように直した:

| linter | 撃たなかった理由 |
|---|---|
| `iface` | 既定で有効なのは `identical` **だけ**。fixture は非公開メソッドを持つ interface 1 つで、どの analyzer も見ない |
| `musttag` | 型ではなく **`json.Marshal` の呼び出し**に撃つ。struct だけでは不可視 |
| `mirror` | `Args` に挙がった引数が**全部** `string(…)` 変換でないと撃たない。片方が文字列リテラルだと黙る |
| `sloglint` | 既定は `no-mixed-args` のみ。key-value と `slog.Attr` を**混ぜる**必要がある |
| `usestdlibvars` | 「GET に見える文字列」ではなく**特定の呼び出し位置**を見る（`http.NewRequest` の第 1 引数、`WriteHeader` の引数） |
| `varnamelen` | 長さだけでなく**距離**。`max-distance` は 5 で、宣言のすぐ隣で使う `i` は黙る |
| `maintidx` | 既定の閾値 20 は数百行の関数でないと下回らない。fixture 側で `under: 100` にした |

そして **7 つを撃たせた時点で `varnamelen` と `maintidx` の桁違いが出た**（上表）。
`maintidx` はクラス A の 6 つ目である。
**「fixture が空である」ことは、欠陥を隠す**。

#### 結果

| | before | after |
|---|---|---|
| golden case | 109 | **193** |
| golden case を持つ linter | 32 / 116 | **116 / 116** |
| isolate で `both == 0` の target | 7 | **0** |
| この tier で見つけた欠陥 | —— | **16**（位置 15 本 ＋ formatter のエラー文言 1 本、原因クラス 9 つ） |
| golden 全体の実行時間 | —— | 68 秒（guff のみ。golden が上流の答えを持っている） |

#### CI で初めて落ちた 2 件 —— 「開発機に入っているから通っていた」

`golines` と `swaggo` は **guff が外部バイナリに shell out する** formatter である。
手元では両方 `~/go/bin` に入っているので 193/193 が緑だったが、
**CI の smoke ジョブは入れていない**（isolate ジョブだけが入れている）ので落ちた。
`PATH` から `go/bin` を外すと手元でも同じように落ちる。

そのときのメッセージがこれだった:

```
guff: golines: ./bad.go: No such file or directory (os error 2)
```

**「ファイルが無い」と読める。無いのは `golines` のほうである。**
`Command::new(bin)` の spawn 失敗を `FormatError::Io { path: filename }` で
包んでいたので、**存在するファイルの名前で「無い」と言っていた**。
`path` を**バイナリ名**に変えた（`golines.rs` / `swaggo.rs`）。
実行ファイルが無いときに読む唯一の行なので、ここが指す先は重要である。

smoke ジョブにも isolate と同じ install ステップを足した。
**入れないという選択は「2 linter を黙って測らない」と同じ**である ——
guff がエラーで落ちるので気付けたが、そこは harness が
「guff failed for <case>」で落とす作りだったからで、
0 件の golden（上記 ginkgolinter）だったら通り抜けていた。

#### CI

golden gate は元から `smoke` ジョブに載っている（guff しか実行しないので速い）ので、
**case を足した分はそのまま CI のカバレッジになる**。加えて
`compat/tests/test_golden_coverage.py` を足した（同じジョブが
`python3 -m unittest discover -s compat/tests` で拾う）:

- **linters.txt の全 linter に golden case があること** ―― 新しい linter が
  golden 無しで入ってきたら、backlog に積まれるのではなく**そこで落ちる**。
- **golden が空でないこと**（上記。`golden-may-be-empty` で明示可）。
- 各 case が `max-issues-per-linter` / `max-same-issues` を書いていること
  （run.sh も見ているが、速いジョブ側でも落ちるように）。
- `sources.txt` の参照先が実在すること。

**次にやること**

1. **深さ**。193 case のうち多くは finding 1 件である。今回は
   「116 全部に桁のゲートを付ける」までで、**各 linter の腕を網羅してはいない** ——
   続き 28 の usetesting（9 腕のうち fixture に居たのは合っている 8 つだけ）が
   その形。fixture を広げるのが次の投資先で、広げた分は両 tier が測る。
2. **`expr_string` を 1 つにする** —— 続き 28 の 2 例目、まだ 3 コピーある。
3. **`compat/results/` を tier ごとに分ける** —— 続き 28 / 29。
4. **gocritic の ruleguard `$`** —— 続き 28。`expr_text` 118 箇所、単独タスク。

---

### 2026-08-24（続き 31）— fixture を広げたら、7 件のうち 2 件は linter ではなく型検査器だった

続き 30 の「次にやること」1。golden は 116 linter 全部に付いたが、
**105 の linter case のうち 75 が finding 2 件以下**で、多くは 1 件だった ——
「桁のゲートを 1 本付けた」であって「その linter が言えることを測った」ではない。

#### 広げた 18 linter（17 → 60 finding）

| linter | before → after | 何が増えたか |
|---|---|---|
| `nilnil` | 1 → 7 | 検査する型の種類ごとに 1 件（ptr / map / chan / func / iface / uintptr） |
| `varnamelen` | 1 → 6 | **5 種類**（variable / constant / parameter / return value / type parameter） |
| `paralleltest` | 1 → 5 | **5 メッセージ**のうち 4 つ（5 つ目は下記） |
| `usestdlibvars` | 2 → 5 | 10 個ある対応表のうち 5 つ |
| `noctx` | 1 → 4 | net/http 系と database/sql 系 |
| `fatcontext` | 1 → 3 | **3 カテゴリ**（loop / func literal / struct pointer） |
| `err113` | 1 → 3 | 定義側と比較側（比較は `==` と `!=` で文言が変わる） |
| `sqlclosecheck` | 1 → 3 | 2 メッセージ（not closed / should use defer） |
| `ireturn` | 1 → 3 | 3 メッセージ（interface / generic interface / of type param） |
| `tparallel` | 1 → 3 | 3 メッセージ |
| `makezero` | 1 → 3 | 2 メッセージ（append 側と `always` 側） |
| `whitespace` | 2 → 3 | 3 メッセージ（leading / trailing / multi-line） |
| `exhaustive` | 1 → 2 | switch と map literal |
| `funlen` | 1 → 2 | 行数と文数（片方だけ超える関数を 2 本） |
| `importas` | 1 → 2 | 別名が違う／別名が無い（別の文） |
| `cyclop` | 1 → 2 | 関数ごとの複雑度とパッケージ平均 |
| `exhaustruct` | 1 → 2 | 単数形と複数形 |
| `errname` | 2 → 2 | 型と sentinel |

**`linter case のうち 2 件以下` は 75 → 64。**

#### 見つかった欠陥 7 件のうち 2 件は `Info.Defs` の穴だった

`varnamelen` の 5 種類のうち 2 つが撃てなかった。**varnamelen は無罪**で、
`Info.Defs` にその識別子が入っていなかった:

1. **関数の中の `const`**。`decl_stmt` の `var` の腕には `record_def` があり、
   **`const` の腕には無かった**。パッケージレベルの定数は resolver 側が記録するので、
   **ファイル直下の `const g` は撃てて、まったく同じ `const c` が関数の中だと見えない**。
   `Defs` から出発する解析すべてに効く穴である。
2. **型パラメータ**。go/types は `declare(scope, id, obj, pos)` の中で
   `recordDef` を呼び、`declareTypeParam` が `name` を渡す。
   guff の `declare` は ident を取らないので、`func f[T any]()` の `T` は
   **`Defs` にどこからも入らない**。

どちらも「linter の腕が 1 本しか無い fixture」に隠れていた。

#### 残り 5 件

- **`fatcontext`**: カテゴリが 2 つしか無く、`check-struct-pointers` /
  `check-loops` / `check-function-literals` の 3 フラグも無かった。
  さらに**ポインタ経由のフィールド代入を「除外」として実装していた** ——
  上流はそれを**カテゴリ**として名前を付け、フラグで落とす。
  除外にすると、上流が見る「本体の最初の nested context」ではなく
  **その次の代入**を報告してしまう。node filter に `FuncDecl` が無かったのも同じ根で、
  上流の 4 種類のうち 1 つを落とすとカテゴリが 1 つ丸ごと消える。
- **`cyclop` のパッケージ平均**: 位置が**パッケージ名**（上流は `File.Pos()` ＝
  `package` キーワード、桁 1）、数値が `12` / `0.5`（上流は `%f` で
  `12.000000` / `0.500000`）。1 行に 2 つ。
- **`sqlclosecheck` の 2 つ目**: 「Close should use defer」も SSA 命令の位置 ＝
  **左括弧**。続き 30 で直した 1 つ目と同じ規約で、同じファイルの別の行だった。
- **`makezero` の `always`**: 未実装。2 メッセージのうち 1 つが存在しなかった。

#### 見つけた形: **足りない腕はたいてい「既定で off の設定」の裏にある**

今回追加した設定は 9 つ —— `check-struct-pointers` / `check-return` /
`check-type-param` / `check-cleanup` / `no-unaliased` / `always` / `multi-if` /
`package-average` / `check: [map]`。
**既定値だけを使う fixture は、既定値だけをテストする。**
`varnamelen` の 2 種類も `fatcontext` の 1 カテゴリも `makezero` の 1 メッセージも、
「設定を書かないと到達できない」ために誰も測っていなかった。

#### 到達できないと分かった腕は、負のケースとして残す

`paralleltest` の 5 つ目
（`Range statement for test %s does not reinitialise the variable %s`）は
**golangci-lint 経由では現代のモジュールから到達できない**:
ラッパが Go バージョン >= 1.22 のとき `ignoreloopVar = true` を立てる
（ループ変数が反復ごとになったので）。guff 側の `DEFERRED` は**値段ゼロ**である。
fixture にはその形を残してある —— 上流が黙るので guff も黙らなければならない。

#### 次にやること

1. **まだ 64 の linter case が 2 件以下**。今回と同じやり方で続けられる:
   上流の `Reportf` を全部数える → 腕ごとに fixture を足す → 既定 off の設定を書く。
2. **`Info.Defs` の穴を横断で見る**。今回の 2 件は varnamelen が偶然見つけた。
   `record_def` の呼び出し漏れが他にもないか、go/types の `declare` 相当と突き合わせる。
3. **`expr_string` を 1 つにする** —— 続き 28 の 2 例目、まだ 3 コピー。
4. **`compat/results/` を tier ごとに分ける** —— 続き 28 / 29 / 30。

---

### 2026-08-24（続き 32）— 20 linter を広げて 4 件。うち 1 件は**単体テストが誤りを主張していた**

続き 31 の続き。同じ手順 —— 上流の `Reportf` を数える、腕ごとに fixture を足す、
既定 off の設定を書く。

#### 広げた 20 linter（21 → 76 finding）

| linter | before → after | |
|---|---|---|
| `sloglint` | 1 → 10 | 12 ルール中 6 |
| `bidichk` | 1 → 9 | **名前が 9 種類**あり、メッセージはその名前を出す |
| `predeclared` | 2 → 9 | func / type / const / var / param / method / ローカル変数 |
| `thelper` | 2 → 6 | 3 メッセージ × 3 subject（t / b / tb） |
| `loggercheck` | 1 → 6 | 3 メッセージ（2 つは既定 off） |
| `funcorder` | 1 → 6 | **6 メッセージ**（下記の 1 つを除く） |
| `testableexamples` | 1 → 5 | package / func / type / method の 4 種 |
| `prealloc` | 4 → 5 | range ループを追加（下記の事故つき） |
| `perfsprint` | 2 → 5 | 13 ルール中 4 |
| `canonicalheader` | 1 → 4 | Get / Set / Add / Del / Values ＋ **負のケース** |
| `tagliatelle` | 1 → 4 | json / yaml / xml / mapstructure |
| `nlreturn` | 1 → 4 | return / break / continue / goto |
| `unqueryvet` | 1 → 4 | リテラル / 定数 / 引数 / raw |
| `musttag` | 1 → 3 | json Marshal / Unmarshal / xml |
| `depguard` | 1 → 3 | deny 側と allow-list 側（別の文） |
| `containedctx` | 1 → 3 | 名前つき / 埋め込み / 無名 struct |
| `dogsled` | 1 → 3 | 個数がメッセージに入るので 3 と 4 は別 |
| `interfacebloat` | 1 → 2 | **埋め込みは 1 と数える**（これを外すと 10 で黙る） |
| `reassign` | 1 → 2 | |
| `noinlineerr` | 1 → 1 | switch / for は**上流が黙る**ので負のケースとして残した |

**linter case のうち 2 件以下は 64 → 52。**

#### 欠陥 4 件

1. **`sloglint` の case 名**。上流は `caseFn(caseName + " case")` ——
   **命名関数を「文」に適用する**ので `snake_case` / `kebab-case` /
   `camelCase` / `PascalCase` になる。guff は `snake case` と出していた。
   原因は guff の case 関数が**空白を語の区切りとして扱っていなかった**こと
   （上流が使う `github.com/ettle/strcase` は扱う）。4 つとも実測して直した。
2. **`loggercheck` の引数描画**。上流は `renderNodeEllipsis` ＝ go/printer で
   印字して 20 runes で切り、`...`（3 点）を付ける。guff は固定の `"…"` を
   出していたので、**どの引数の話か読み手に分からなかった**。
3. **`canonicalheader` の偽陽性**（下記）。
4. **`funcorder` の 7 件目**（下記。欠陥ではなくピンとのズレ）。

#### `canonicalheader` —— 単体テストが偽陽性を要求していた

`h.Get("etag")` に guff は `instead use: "ETag"` を出す。**上流は何も言わない。**

```go
headerKeyCanonical, isWellKnown := canonicalHeaderKey(argValue, wellKnownHeaders)
if argValue == headerKeyCanonical || isWellKnown {
    return
}
```

initialism の表（`Etag` → `ETag`、`X-Request-Id` → `X-Request-ID` …）は
**抑制にしか使われない**。MIME 正規形が表にあれば上流は黙る。
guff は表の値を「提案」として出していた。

**そして `crates/guff-style/tests/checks_test.rs` がその誤りを assert していた**
（`instead use: "ETag"` を要求）。どの tier もそれを否定できなかった ——
isolate fixture は `content-type` だけで linter に届いていたからである。
§1 が 2,848 件の crate テストについて言っていることの実例:
**「guff が撃つ」の確認は「正しく撃つ」の確認ではない。**
テストを黙ることの assert に直し、fixture に 3 つの負のケースを足した。

#### `funcorder` の 7 件目 —— ピンに無い設定

guff は `funcorder.function` を解釈する。**golangci-lint 2.12.2 の
`FuncOrderSettings` にそのキーは無い**（ピンより後に入った）。
同梱の funcorder v0.6.0 にはチェック自体があるが、2.12.2 には**有効にする手段が無く**、
キーを黙って無視する。つまり guff が**ピンより先に進んでいる**。
fixture から設定を外し、形は負のケースとして残した。
ピンが追いつくのを見るのは `compat/drift.py` の仕事である。

**上流のソースを読むときはピンのタグを読むこと。**
`~/projects/src/github.com/golangci/golangci-lint` は HEAD なので、
`git show v2.12.2:pkg/config/linters_settings.go` でないと今回の差は見えない。

#### 事故: **広げるつもりで置き換えていた** 2 件

`prealloc` と `funlen` の fixture を**書き直して**しまい、
`prealloc` の golden は 4 → 2 に**減った**。消えたのは
go/printer の優先順位描画（`len(a)/2 + len(b)` は `/` の空白が落ちる ——
dapr の `pkg/runtime/hotreload/differ` が実例）を pin していた 4 つで、
これは case の説明文に「置き換えるな、広げろ」と自分で書いた直後の話である。

見つけ方は**セッション開始時のコミットと関数の数を突き合わせる**だけだった:

```bash
git show ceb40019:compat/isolate/fixtures/<l>/bad.go | grep -c '^func '
```

広げた fixture すべてでこれを回し、減っている 2 件を復元した
（`prealloc` は 4 → 5、`funlen` は元の空関数の回帰ケースを戻した）。
**広げる作業には「減っていないこと」の確認が要る。**

#### 次にやること

1. **まだ 52 の linter case が 2 件以下。** 同じ手順で続く。
2. **`Info.Defs` の穴を横断で見る**（続き 31 の 2）。
3. **`expr_string` を 1 つにする** —— 続き 28 の 2 例目。
4. **`compat/results/` を tier ごとに分ける** —— 続き 28 / 29 / 30。

---

### 2026-08-24（続き 33）— さらに 33 linter。**「1 件のままが正しい」も結論である**

続き 32 の続き。同じ手順を残りに当てた。

#### 広げた 33 linter（合計 82 linter、finding 2 件以下の case は 75 → 28）

| linter | before → after | | linter | before → after |
|---|---|---|---|---|
| `dupword` | 2 → 7 | | `errname` | 2 → 4 |
| `promlinter` | 1 → 5 | | `exhaustruct` | 2 → 4 |
| `inamedparam` | 2 → 5 | | `unconvert` | 1 → 4 |
| `iface` | 2 → 5 | | `asciicheck` | 1 → 4 |
| `gochecknoglobals` | 1 → 4 | | `copyloopvar` | 2 → 4 |
| `exptostd` | 2 → 4 | | `cyclop` | 2 → 3 |
| `durationcheck` | 1 → 3 | | `goconst` | 2 → 3 |
| `embeddedstructfieldcheck` | 1 → 3 | | `exhaustive` | 2 → 3 |
| `gosmopolitan` | 2 → 3 | | `iotamixing` | 1 → 3 |
| `recvcheck` | 2 → 3 | | 他 12 本 | 1 → 2 |

#### 欠陥: `exhaustive` の**並び順**

```
golden  missing cases in switch of type p.Size: p.Medium, p.Large
guff    missing cases in switch of type p.Size: p.Large, p.Medium
```

guff は `missing.sort()`（アルファベット順）。上流は残ったメンバを定数値でグループ化し、
**グループの中もグループ同士も `astBefore`（宣言順）で並べる**。
集合は同じで**文が違う**。
**メンバが 2 つ欠けた switch でないと見えない** —— 1 つ欠けの fixture では
順序という概念が現れないからである。

#### 欠陥: `gomoddirectives` は 4 種類を**全部 go.mod の 1 行目**に置いていた

`exclude` / `toolchain` / `tool` / `godebug` の 4 つが `line_pos(1)` ——
つまり `module` 行 —— で報告されていた。go.mod のパーサは行番号を
クロージャに渡しておきながら `let _ = ln;` で捨てていた。

**1 件の fixture では原理的に見えない**: finding が 1 つしか無いとき、
その行が「何に対して」間違っているかが存在しない。
directive を 2 つ書いた瞬間に**2 件が同じ行に載り**、それは正しくありえない。

go.mod の 1 行 directive に `Directive { value, line }` を持たせて 4 つとも直した。
`gomoddirectives` は 1 → 4 件（local replace / module replace / exclude / retract）。

#### 「1 件のままが正しい」3 件

広げても finding が増えなかったものがある。**これは失敗ではなく測定結果**で、
fixture のコメントをそちらに直した。

- `golines` は**ファイルごとに 1 件**（`File is not properly formatted`）。
  formatter は「直す場所ごと」には報告しない。
- `gomodguard` / `gomodguard_v2` は **`import` 文ごとに 1 件**。
  ブロックされたモジュールを 2 回呼んでも 2 件にはならない。
- `testpackage` / `asasalint` も同様に 1 件で正しい。

**自分の仮説がコメントに書いてあるとき、走らせた結果がそれを否定したら、
コメントのほうを直す。**

#### stub が足りないと golden は**型エラーを 1 件として記録する**

`zerologlint` の fixture に `log.Error().Int(...)` を足したら、golden に

```
bad.go:1:0:typecheck::: # example.com/zerologlint\n./bad.go:12:14: log.Error().Int undefined …
```

が入った。**これは tier の自衛が働いた形**である ——
続き 30 で足した「空の golden を落とす」ガードと guff の非ゼロ終了に続いて 3 つ目で、
**stub の不足は沈黙ではなく finding として出る**。stub に `Int` / `Bool` を足した。

#### 「減っていないこと」の確認を毎回やった

続き 32 で fixture を 2 本置き換えてしまった反省から、バッチごとに回した:

```bash
git show <base>:compat/isolate/fixtures/<l>/bad.go | grep -cE '^(func|type|const|var) '
```

61 fixture を突き合わせて**減少ゼロ**。

#### 次にやること

1. **残り 27 の linter case**。ただしここから先は「1 件が正しい」ものが増える
   （`gomoddirectives` / `swaggo` / `mirror` / `noinlineerr` …）ので、
   **数を追うのではなく上流の `Reportf` を数えて突き合わせる**こと。
2. **`Info.Defs` の穴を横断で見る**（続き 31 の 2）。
3. **`expr_string` を 1 つにする** —— 続き 28 の 2 例目。
4. **`compat/results/` を tier ごとに分ける** —— 続き 28 / 29 / 30。

---


---


---


---


---

## 5. 既知の「暗黙 allowlist」台帳

`compat/normalize.py` が消している差分。Phase 3 の golden tier では正規化しないので、
ここに挙げたものは**個別に潰す or 恒久的な非互換として理由付きで記録する**必要がある。

**2026-08-13（15 本目）に全行を測った。** 「未調査」と「まだ必要」は別の主張で、
行を正当化するのは後者だけである。測り方は 1 つ —— **既存の実行の finding 集合を、
正規化を 1 つずつ切って鍵付け直し、差分が増えるかを見る**。増えなければその正規化は
何も隠していないので消せる。増えたなら、増えた分がそのまま読むべき finding の一覧になる。

| # | 対象 | 正規化が消しているもの | 切ったときの差分 | 状態 |
|---|------|------------------------|---:|------|
| 1 | errcheck | callee 名を含む形 (`Error return value of \`f\` is not checked`) と含まない形 | 1 | **解消 2026-08-11（6 本目）**。表記ゆれではなく実装の食い違いだった: guff は常に `FullName()` を出しており、上流は**セレクタでない呼び出しに名前を付けない**／付けるときも**書かれたとおりの綴り**を使う。`cases/errcheck` が正規化なしで比較する。vault に 1 件だけ残っており未診断 |
| 2 | unused | メッセージ先頭の prefix / メソッド修飾 | 12 | **解消 2026-08-13（15 本目）**。バグを隠していた: guff は honnef が名前の前に置く種別語（`lintcmd/lint.go` の `"%s %s is unused"`）を出さず、値レシーバを `(T).M` と括弧で包んでいた（上流は `*` を出したときだけ包む）。正規化を削除 |
| 3 | staticcheck | `SA1234: ` チェックコードを**両側から**剥がす → コード取り違えが不可視 | 1 | **測定済み**。隠れているのは #4 の 1 件だけ。据え置き |
| 4 | staticcheck | QF1011「could omit type」/ ST1023「should omit type」の言い回し | 1 | **測定済み**。同じ行を**コードも文言も違う 2 チェック**が撃つ実在の差。据え置き |
| 5 | staticcheck | Deprecated 文の末尾ピリオド有無 | 0 | **解消 2026-08-13（15 本目）**。何も隠していなかった。正規化を削除 |
| 6 | modernize | チェック名 prefix | 2 | **解消 2026-08-13（15 本目）**。バグを隠していた: 25 チェックのうち 2 つだけが `Diagnostic::category` を設定し、残りは空だった。中央で押すようにした |
| 7a | govet | pass 名 prefix | 0 | **解消 2026-08-13（15 本目）**。何も隠していなかった。正規化を削除 |
| 7b | govet | `(declared using go1.X.Y)` のパッチバージョン | 22 | 意図的（環境差 —— golangci は自分のビルドに使った Go の版を出す） |

### 明示的な allowlist（`compat/allowlists/`）

上の表は「正規化が黙って消しているもの」。こちらは**ファイルに書いてある**もの。
`--update-allowlist` はファイルのコメントを消してしまうので、**理由はここが正典**。

| 対象 | 件数 | key | 理由 | 記録日 |
|------|-----:|-----|------|--------|
| ~~consul~~ | ~~1~~ | ~~`agent/consul/catalog_endpoint.go:280` SA5011~~ | **解消 2026-08-13（15 本目）**。σ の**もう一方の向き**（参照外しと nil 検査の**間**にある分岐が値を改名する）を `renamed_before_check` で入れた。同日に見つかった 2 件目（`agent/xds/listeners_ingress.go:227`）も同時に消えた | 2026-08-09 / 解消 2026-08-13 |
| consul | 2 | `agent/event_endpoint_test.go:115` / `agent/http_test.go:1728` SA9008 | 上流の IR 検証（`ValueForExpr` + `irutil.Flatten`）未移植。パターン自体は一致済み。誤検出。§4 の 2026-08-09（2 本目）に最小再現。 | 2026-08-09 |
| controller-runtime | 9 | `compat/allowlists/controller-runtime.txt` 参照 | ill-typed が 16 → 0 になった日（14 本目）に見えるようになった precision の穴。**17 件だったものが 15 本目で 9 件に**（SA9003 の 6 件は `irutil.IsExample`、SA1019 の 2 件は非推奨 fact の欠落）。残りは unparam 2（`MakeInterface` にオペランドが無い＝§7 と同根）、その症状としての nolintlint 5、nilerr 1、bodyclose 1 | 2026-08-13 |

これ以外の allowlist ファイルは**すべてヘッダのみ（0 件）**。記録するのは
`oss-nightly` / weekly を CI ゲートにするため — 恒久的に赤いゲートは次の劣化に
日付を付けられない。**残りを消すのが Phase 3 の残タスク**であり、
消えたらこの節ごと削ること。

加えて、`issue_key` が **column / severity / SuggestedFix を比較していない**（§1）。
うち **column と severity は golden tier（`compat/golden/`）が比較するようになった**が、
それはゴールデンを持つ check に限る。gocritic では実際に 42 件の column バグが出た（§4）ので、
**残りの linter にも同種のバグがあると考えるのが妥当**。SuggestedFix は依然どこも比較していない。

---

## 6. 恒久的に観測できない check

ゴールデンでも OSS でも原理的に捕まえられないもの。「未着手」ではなく「不可能」として記録する。

**2026-08-11（5 本目）以降、`docs/COVERAGE.md` の `never` はこの表と一致する。**
（例外は最終行の `govet/asmdecl` —— **まだ実装していない**ので台帳の母数に入らない。
ここに載っているのは「実装しても観測できない」ことが実測で分かっているからで、
実装するかどうかの判断材料としてこの表に置いてある。）
「まだ載せていない」check はもう無い。ここに 1 行足すときは、
**上流に食わせて 0 件であることを実測してから**書くこと ——
`govet/framepointer` の行は「GOARCH がホスト依存だから」という**推測**のまま
1 セッション残り、実測したら理由が違っていた。

| check | 理由 |
|-------|------|
| `gocritic/whyNoLint` | 説明のない `//nolint` を報告する checker だが、その `//nolint` 自身が同じ行の findings を抑止するため、golangci-lint の出力に現れない（上流に食わせても 0 件）。単体テストでのみ検証可能。 |
| `govet/framepointer` | **golangci-lint は `.s` ファイルの診断を 1 件も出さない**。同じ fixture に `go vet` を食わせると framepointer 2 件 + asmdecl 4 件が出るのに、golangci-lint 2.12.2 は 0 件（`GOARCH` を合わせても、ホスト arch のままでも同じ）。**この行の以前の理由（GOARCH がホスト依存だから）は誤り**で、ケース単位の環境変数を入れても解けない — その仕組み自体は 2026-08-11（2 本目）で入れてあり、`SA1027` はそれで回収できた。単体テストでのみ検証可能。 |
| `govet/cgocall` | `import "C"` を含むファイルが要る。cgo と C コンパイラを CI ゲートの前提にしたくない。単体テストでのみ検証可能。 |
| `govet/asmdecl`（**未実装**） | `framepointer` と同じ理由で、実装しても観測できない。実測（2026-08-13）: 引数サイズを意図的に間違えた `a_arm64.s` に `go vet` は `[arm64] Add: wrong argument size 8; expected $...-24` を出すが、golangci-lint 2.12.2 は **0 件**。**golangci-lint は `.s` の診断を 1 件も出さない**。移植の是非とは別に、ゲートに載せる方法が無い。 |

### 意図的な非互換: revive の importer 盲目には追従しない `[決定 2026-08-10]`

**方針: 真陽性は捨てない。この 3 件は恒久的な差分として据え置く。**

revive は `types.Config{Importer: importer.Default()}` で型検査する。
`importer.Default()` は gc の export data importer で、いまの Go には `.a` が無いため
**import が全部 invalid に落ちる**。したがって「別パッケージで宣言された型」を要する
rule は上流では**常に黙る**。guff は全プログラムの型情報を持つので正しく答えてしまう。

| golden の差分 | 上流が黙る理由 |
|---|---|
| `time-equal`（extra, `extended_bad.go:73`） | `TypeOf(x)` が `time.Time` かを見るが invalid が返る |
| `epoch-naming`（extra, `extended_bad.go:428`） | 同上（`t.Unix()` のレシーバ型） |
| `time-naming`（extra, `bad.go:50`）`[追加 2026-08-11（2 本目）]` | 同上（`TypeOf(name)` が `time.Duration` か）。guff 側はこの rule が**そもそも死んでいた**ので、直した結果ここに並んだ。§4 参照 |
| `context-keys-type`（missing/extra の対, `bad.go:65`） | `context.WithValue` のシグネチャが解決できず、untyped 定数が `string` に defaulting されない。文言が `untyped string` と `string` で割れる |

**追従すると `time-equal` / `epoch-naming` / `time-naming` が丸ごと死ぬ。** どれも実在のバグを
指す rule なので、上流の欠陥を再現するために真陽性を捨てるのは割に合わないと判断した。
`cases/revive/ratchet.json` の 1/4 は**到達目標ではなく固定の床**であり、
**これ以外の差分が 1 件でも増えたらそれはバグ**。

`unhandled-error` だけは例外的に上流に合わせてある（`callee_is_local`、
§4 の 2026-08-10 1 本目）。あちらは上流が 0 件・guff が 22 件で、
**差が大きすぎて golden ケース全体のノイズになる**ためで、方針が違うわけではない。
上流が importer を直したら（`go/packages` へ移行するなど）この節ごと消えるので、
revive のバージョンを上げるときに再確認すること。

---

## 7. アーキテクチャの違いで再現できないもの

§6 が「上流に食わせても観測できない」なら、こちらは「観測はできるが guff の
構造上そのままでは再現できない」。**allowlist ではなく、代償を明記した設計判断**として記録する。

### ~~`_ = f()` の arity 不一致を型検査していない~~ `[記録 2026-08-11 / 解消 2026-08-11（2 本目）]`

**解消済み。** `is_call` 分岐と `single_value` を入れた。詳細は §4 の
2026-08-11（2 本目）。以下は当時の記録。

**これは設計判断ではなく単なる欠落**なので、直すべきものとしてここに置く
（§4 の 2026-08-11 の「次にやること 1」）。

```go
func two() (int, error) { return 0, nil }
_ = two()      // go build: assignment mismatch: 1 variable but two returns 2 values
x := two()     // 同上
```

`go build` は両方を落とすが、guff は**エラーを 1 件も出さずに解析を続ける**。
`crates/guff-types/src/check_assign.rs` の `assign_vars` / `init_vars` が
`r == 1 && l != 1` のときだけ `eval_multi` に入るためで、`l == r == 1` で
右辺が tuple のときは `l == r` の枝を素通りする。go/types は `exprList` で
**l に関係なく**多値を展開してから数を比べるので、この形も捕まる。

影響は finding 1 件では済まない。**ill-typed かどうかはパッケージ単位の
スイッチ**で、golangci-lint 側はこのパッケージを typecheck エラーとして
他の findings を落とす。guff は落とさない。Phase 1 のゲートが数えているのは
まさにこの差である。

見つかった経緯そのものが教訓で、`testdata/gosec/bad.go` は
**3 箇所この形を含んだまま何ヶ月も緑だった**。Rust のテストハーネスは
ill-typed を warning で流し、guff の型検査器は気付かない。
**実 Go ツールチェインに一度も読ませていない fixture は、こうなる。**

### 再帰の深さ — goroutine スタックは伸びる（SA1001）`[記録 2026-08-10]`

`gostd::template` は再帰下降パーサで、Rust のスレッドスタックは**固定長**。
Go は goroutine スタックが伸びるので `{{if}}` を 10 万段ネストしても普通に parse する
（上流が深さを制限しているのは**括弧付きパイプラインだけ**で、値は 10000）。

実測: 1 段あたり release で約 1 KiB / debug で約 4 KiB。**制限を入れる前は
2 MiB スタックの release ビルドで括弧 1,000 段が abort した**。guff の lint ワーカーは
8 MiB だが、深さは入力次第でいくらでも増えるので上限が無ければいつか踏む。
そして踏んだときの結果は**プロセス abort** — Phase 1 が「差分に出ない失敗」として
常時 fail 扱いにしている worker panic より更に悪い。

そこで `MAX_RECURSION = 250` で打ち切る。超えたときは
`guff: template nesting exceeds guff's recursion limit` を返す ——
**このモジュールが出す唯一の「Go には存在しない文字列」**であり、
`unexpected` も `bad character` も含まないので **SA1001 は黙る**。
代償は「250 段より深いテンプレートで上流が撃つ finding を撃たない」ことだが、
実在のテンプレートは 1 桁段しかネストしない。
`tests/gostd_template.rs` が **2 MiB スレッド（本番の 1/4）で 10 万段**を回して
abort しないことを固定している。

### Rust の `String` は不正な UTF-8 を持てない（SA1000 の `Expr`）`[記録 2026-08-11（3 本目）]`

**3 セッション積み残していた「goregexp の 202 行の end-to-end 確認」の答え。**
結論から言うと**移植は 202/202 合っている**。残るのは 1 点、しかも構造的なもの。

やったこと: オラクルの 202 行（`ErrInvalidUTF8`）のパターンを全部
`regexp.MustCompile("\xNN…")` として 1 ファイルに書き出し（バイトは全部 `\xNN`
エスケープなので `.go` 自体は正しい UTF-8）、**text 出力**で両ツールをバイト比較した。
JSON では駄目で、その理由が本項の中身になる。

| 観測 | 結果 |
|---|---|
| finding 数 | 202 / 202 |
| file:line:col | 全行一致 |
| メッセージ | **`Expr` の描画以外は全行一致** |
| `Expr` の中身 | golangci は**生バイト**（`` `\xff` ``）、guff は **U+FFFD** |

`syntax.Error.Expr` は「不正になった以降のパターンの生 slice」なので、
Go の `string` はそのまま持てるが **Rust の `String` は持てない**。
`Diagnostic` を `Vec<u8>` にしない限り再現できない。

**ただし置換の粒度は合っている。** guff は**バイト 1 個につき U+FFFD 1 個**を出す。
Go の `encoding/json` も同じ（`utf8.DecodeRune` が失敗したら 1 バイト進めて
`�` を書く）ので、**JSON を通す経路ではむしろ一致する** ——
golden tier が両側 JSON なのはそのためで、この差はそこには出ない。
出るのは golangci の text 出力だけで、あちらは生バイトを素通しする。

**202 行のうち 189 行は、golangci の出力を lossy デコードしても一致する**
（1 バイトの不正 = 1 個の U+FFFD）。残り 13 行は `\xe2\x82` のような
**途中で切れた多バイト列**で、Python や Go の「maximal subpart」規則が
U+FFFD を 1 個にするのに対し guff（と Go の json）は 2 個にする、という
**デコーダ側の規則の違い**であって guff の側の誤りではない。

fixture には golden で区別できる形だけ足した（`\xc3` = 1 バイト、
`\xe2\x82` = 2 バイト、末尾に演算子が付く 2 形）。202 行を全部足しても
golden 上は同じ `` の列になるだけで情報が増えない。

### 再帰の深さ、二度目 — `factor` は木の高さでは抑えられない（SA1000）`[記録 2026-08-10]`

SA1001 と同じ問題だが、**上限を 1 つにすると成立しない**ことが分かったので分けて記録する。
`gostd::regexp` の再帰は 2 種類あり、**コストも到達条件も違う**。

| 再帰 | 1 段のコスト | 何が抑えるか |
|---|---|---|
| `factor` → `collapse` → `factor` | **debug 実測で 600 段が 2 MiB を溢れさせる**（Vec を数本持つ） | **何も抑えない**。共通リテラル接頭辞 1 rune につき 1 段潜り、Go の `maxHeight` は木を建てる**上り**でしか効かない |
| `calcSize` / `calcHeight` / `Equal` / `repeatIsValid` | 局所変数数個 | Go の `maxHeight`（1000）。ただし**上限がそれ未満だと不一致になる** |

したがって `MAX_FACTOR_DEPTH = 250` / `MAX_WALK_DEPTH = 2000`。
後者を 1000 より大きく取らないと `(((…1001 段…)))` が
**Go では `expression nests too deeply` なのに guff は黙る**。

超えたときは `CompileResult::Undecided` を返し、**SA1000 は何も報告しない**。
SA1001 が使った「Go に存在しない文字列を返す」逃げ道は使えない ——
SA1000 は `regexp.Compile` の error を**全部**報告するので、whitelist の外側が無い。

代償は「接頭辞連鎖が 250 段より深いパターンで上流が撃つ finding を撃たない」ことだけ
（**誤検出は増えない**）。実在の交替は接頭辞を数 rune しか共有しない。
なお `a|aa|aaa|…` は n ≈ 8190 を越えると rune 予算の方が先に効くので、そこから先は再び一致する。

### ~~Go の文字列定数はバイト列、guff の定数は `String`~~ `[記録 2026-08-10 / 解消 2026-08-10（5 本目）]`

**解消済み。**§4 の 2026-08-10（5 本目）を参照。ここに残すのは、これが
「アーキテクチャの違いで再現できない」ものだと**一度は判断された**という記録のためで、
実際には**単に guff 側の表現の誤り**だった。§7 に入れる前に「本当に直せないのか」を
問う理由がこれである。

当時の記述: Go の `string` は**バイト列**で、`"\xff"` は 1 バイトの 0xFF。
guff は `guff-constant` の `Value::String(Arc<String>)` ＝ Rust の `String`（= rune 列）で
持つので、`parse_string_lit` は `\xff` を**コードポイント U+00FF**（UTF-8 で 2 バイト）に
してしまう —— という診断そのものは正しかった。誤っていたのは「直す場所が無い」の側で、
`Value::String` を `Arc<Vec<u8>>` にするだけで済んだ。

### 依存パッケージを跨ぐ purity 推論（SA4017）

上流の `analysis/facts/purity` は**解析するすべてのパッケージ**（stdlib を含む依存も）で
関数本体を見て純粋性を推論し、object fact として伝播する。`pureStdlib` の表は
`check` の内部でしか参照されないので、`strings.TrimSpace` が pure なのは
「表に載っているから」ではなく「`strings` パッケージを解析したときに fact が
書き出されたから」である。

guff は **root パッケージの関数本体しか IR 化しない**
（`ssautil::load::build_package_for_analysis` は依存にはメンバの殻しか作らない）。
依存の body が無いので推論しようがない。したがって guff は表を**呼び出し側でも**
引く形に読み替えている（`purity::PurityResult::is_pure`）。表に載っている名前については
上流の推論も同じ表で短絡するので**結果は完全に一致する**。

一致しないのは、**上流が跨ぎで推論した**純粋性だけ:

| 例 | 上流が pure と判定する理由 |
|---|---|
| `strings.ReplaceAll` | 本体が `strings.Replace`（表にある）を呼ぶだけ |
| `net/http.StatusText` | 本体が定数を返す switch のみ |
| ユーザ定義パッケージの `errors.New` 相当 | 同上、同一モジュール内の依存を解析して fact 化 |

現在の golden の missing 12 件がこれ。解消するには依存パッケージにも SSA を
構築して analyzer を走らせる必要があり、prometheus 規模では peak RSS / 実行時間の
桁が変わる。**やるなら Phase 5（コーパス多様化）とセットで性能を測ってからにすること。**

### SA5011 の σ（sigma）ノード — と、そこから波及する SrcFuncs のメソッド

honnef の `go/ir` は **SSI 形式**で、条件分岐のたびに値を σ ノードで分割する。
SA5011 はこれに全面的に依存していて、`if x == nil` の被演算子を `maybeNil` に登録し、
deref 命令のオペランドが**その IR 値と同一か**だけを見る（上流のコメント曰く
「極めて素朴な検査。phi も sigma も情報を伝播しない」）。σ があるおかげで

```go
if cached { _ = ce.ref }   // ここの ce は σ 値
…
if ce != nil { … }         // こちらは別の値 → 一致しない → 報告しない
```

という形が**自動的に偽陽性にならない**。**guff-ssa は go/ssa 移植なので σ ノードが無い**。
同じ形で `ce` が単一の値になり、guff は撃ってしまう。prometheus の
`scrape/scrape.go:1709-1711` ほか計 6 件がこれ（2026-08-08 §4）。

なお `hints != nil && hints.ShardCount > 0` の側は**別問題**で、17 本目が構文側の
当て木（`short_circuit_guarded_derefs`）を当てていたもの。18 本目に
`logicalBinop` を移植して当て木を削除した（§4 の 2026-08-14）。σ が無い件とは違い、
これは CFG が足りていなかっただけである。

**σ だけではない — honnef IR には単一の `Exit` ブロックがある（2026-08-20 追記）**。
`return` は結果をセルへ Store して `f.Exit` へ Jump する形に落ちるので、
結果を持たない関数の `return` は**ただの Jump 1 命令のブロック**になる。すると
`jumpThreading` が畳み、`a.Succs[0] == a.Succs[1]` になった `If` は Jump に置き換えられる
（`go/ir/blockopt.go:94`）。つまり `if p == nil { return }` が関数の末尾にあると
**`If` が消えて `maybeNil` に何も登録されない** —— その `if` より上にある deref も報告されない。
go/ssa 移植の guff は `Return` を各ブロックに直接置くので b が「ただの Jump」にならず、
`If` が残り、上の deref を撃つ。σ を入れるにせよ入れないにせよ、**この差だけは別に埋める必要がある**
（実測表は §4 の 2026-08-20（続き 3））。

波及として、**`buildir` の `SrcFuncs` に既定でメソッドを入れられない**。
上流は常に入れるが、入れた瞬間にこの SA5011 偽陽性がメソッド本体から噴き出して
regress ゲートが落ちる。現状は
`BuildIrResult::src_funcs_with_methods()` で**チェック単位のオプトイン**にしてある
（SA4017 のみ）。**src_funcs を回す他の 20 以上の analyzer はメソッドを見ていない
＝ 静かな recall 損失が残っている。** 解くには SA5011 に σ 相当の手当て
（分岐をまたぐ値の区別）を入れるのが先。

### ~~`MakeInterface` がオペランドを持たない~~ → **発行されていなかった**（SA4006／unparam）`[記録 2026-08-08 / 解消 2026-08-14（18 本目）/ 残り 1 件は下記 `emitCallArgs`]`

**解消済み、ただし診断が間違っていた。** 15 本目から 17 本目まで
「`pub struct MakeInterface {}` がボクシングされる値を持たない」と記録してきたが、
オペランドを足しても `sa4006/ok.go` の偽陽性は消えなかった。原因は 1 段下にあり、
**`emit_store` が値を素通しで格納していた** —— go/ssa の `emitStore` は
`Val: emitConv(f, val, MustDeref(addr.Type()))` で、そこが**インターフェースへの
ボクシングが起きる唯一の場所**である。空構造体だったのは症状であって原因ではない。
詳細は §4 の 2026-08-14（18 本目）。SA4006 の extra 1 件は消え、
golden の staticcheck-sa ratchet は extra 7 → 6 になった。

**unparam の 2 件はまだ閉じていない。** 下記 `emitCallArgs` を参照。

### ~~`emitCallArgs` が無い —— 呼び出し引数が仮引数型へ変換されない~~ `[記録 2026-08-14（18 本目）/ 解消 2026-08-14（19 本目）]`

**解消済み。** `builder/call.rs` の `set_call` / `emit_call` が共通の
`emit_call_args` を通り、実引数を仮引数型へ `emit_conv` する。
可変長引数のスライス構築は**意図的に移植していない**（guff は個別に渡して
`CallCommon::ellipsis` で spread を記録する既存の規約があるため）ので、
末尾は可変長仮引数の**要素型**へ変換する。多値の連鎖（`f(g())`）は実引数と
仮引数の個数が合わないので変換しない —— 上流は `emitExtract` で平らにする。
入れた直後に golden が `isValuePreserving` の欠落（チャネル間・ポインタ間の変換が
`Convert` になっていた）を出したので、それも移植した。詳細は §4 の 2026-08-14（19 本目）。
`compat/allowlists/controller-runtime.txt` の unparam 2 件はこれで閉じた。
以下は当時の記録。


`builder/call.rs` の `c.args.push(self.expr(arg))` は引数をそのまま積む。
go/ssa の `emitCallArgs` は通常引数ぶん
`emitConv(fn, args[i], sig.Params().At(i).Type())` を回し、可変長引数はスライスに詰め直す。
実測:

```go
func take(i I) {}
func viaArg(t T)    { take(t) }                 // guff: t0 = take(t)
func viaAssign(t T) { var i I = t; take(i) }    // guff: t0 = make I <- T (t) / t1 = take(t0)
```

go/ssa は `viaArg` にも `t0 = make I <- T (t)` を出す。

見えるのは `compat/allowlists/controller-runtime.txt` の unparam 2 件と、
その `//nolint:unparam` が「未使用」に見える nolintlint 1 件。
上流 unparam は `addImplementing(findNamed(instr.X.Type()), iface)` を
**全 `MakeInterface` について**回して「この名前付き型が実装しなければならない
メソッド名」の表を作るが、実際の変換地点（`WithValidator(&podValidator{})`）が
**呼び出しの引数**なので、guff には命令が無く表が空になる。
18 本目に SSA 版の `typesImplementing` を書いて回したが観測可能な差が 0 だったため、
buildir 依存だけを増やすことになるので**入れずに戻した**。先にこちらを入れること。

**allowlist の「SSA を 1 つ直すと 4 件が同時に消える」は正しくない。2 つである** ——
`MakeInterface` のオペランド（済）と `emitCallArgs`（未）。

**［19 本目で解消］** `emitCallArgs` を入れ、`typesImplementing` を入れ直したら
表が埋まり、unparam 2 件が閉じた。ただし**4 件目（nolintlint 1 件）は閉じなかった** ——
つまり「4 件」の内訳自体が間違っていて、あの nolintlint は別の原因である。

### ~~インターフェースのメソッドにレシーバが繋がっていない~~ `[記録 2026-08-11（6 本目）/ 解消 2026-08-14（18 本目）]`

**解消済み。** `interface_set_method_receivers` /
`interface_repoint_method_receivers` を足し、`Checker::interface_type`（インターフェース
自身）・`Checker::type_decl`（`type T interface{…}` のとき名前付き型へ付け替え）・
`ureader`（インターフェース自身）の 3 か所で呼ぶ。golden の errcheck-verbose ratchet
1/1 を削除した。

**上流自身が 2 通りに綴る**、というのがこの件の要だった: ソース検査した
`type T interface{…}` は `(pkg.T).M`（`Checker.interfaceType` が `def` を受け取る）、
export data から読んだものは `(interface).M`（`ureader` はレシーバ無しで作り、
`types.NewInterfaceType` がインターフェース自身を入れ、`writeFuncName` は
`*types.Interface` を見ると型を書かず `interface` と綴る）。
したがって errcheck の別名は**片方だけ**消えた: `pkg.M` は削除、`(interface).M` は残す ——
後者は回避策ではなく、上流 errcheck が `namesForExcludeCheck` /
`walkThroughEmbeddedInterfaces` で**選択の受け手型から**名前を組み立てていることの代役である。

残っているのは `subst.rs` の `replaceRecvType`: インスタンス化したジェネリック
インターフェースのメソッドが、レシーバとして**元の**インターフェースを指したままになる。
上流は Func と Signature を複製する（メソッドはインスタンス間で共有されるため）。
読むのは名前だけ（`identical` はレシーバを見ない）なので優先度は低い。

### ~~型集合を見ない `allX` と、ジェネリック型エイリアス~~ `[記録 2026-08-12（11 本目）/ 解消 2026-08-12（12 本目）]`

**解消済み。** `predicates.rs` に `all_basic` と `allX` 7 本を入れて演算子・文・
組み込み関数の 12 箇所を差し替え、untyped 定数の型パラメータへの変換
（`underIs`）と go1.24 のジェネリック型エイリアスも入れた。詳細は §4 の
2026-08-12（12 本目）。以下は当時の記録。

**これは設計判断ではなく欠落**なので、直すべき側に置く。どちらも
`go build` が通るコードで **guff の型検査だけが落ち**、パッケージ全体が ill-typed になる
＝ 型に依存する analyzer が丸ごと黙る（Phase 1 が数えているのがまさにこれ）。

```go
type Box[T any] struct{ v T }
type Alias[T any] = Box[T]        // guff: undefined: T

type Number interface{ ~int | ~float64 }
func Sum[T Number](xs []T) T {
    var total T
    for _, x := range xs { total += x }              // guff: operator ADD not defined on operand
    return total
}
func Less[T Number](a, b T) bool { return a < b }    // guff: operator LSS not defined on operands
```

`crates/guff-types/src/predicates.rs` は自分でこう書いている ——
「These look at `t.Underlying()`; they don't look inside type parameters
(matching Go's `isX` family). Type-set-aware variants (`allX`) are deferred」。
上流の `Checker.binary` が引くのは `allNumeric` / `allOrdered`、
つまり**型集合の全項が満たすか**のほうである。
したがって現状、**ジェネリックな算術・比較を書くパッケージは丸ごと解析されない**。

エイリアスのほうは Go 1.24 の「型パラメータを持つエイリアス」で、
`corpus/shapes.py` の `genericalias` が**どのターゲットでも 0**と測っている形。
`compat/golden/cases/generics` はこの 2 形を避けて書いてあり、
避けた理由は fixture 自身のコメントにも残してある。型検査が通るようになったら戻すこと。
（12 本目で戻した。`genericalias` は `EXCLUDED` に「fixture で埋める」と記録した。）

### ~~`range` / 送受信が `commonUnder` を見ない~~ `[記録 2026-08-12（11 本目）/ 解消 2026-08-13（15 本目）]`

**解消済み。** `stmt.rs` の `range_key_val` と `send_chan_elem`、`expr.rs` の
チャネル受信を `crate::under::common_under` に載せ替えた。
`crates/guff-types/tests/generic_ops.rs` の `#[ignore]` を外してある。

これも `allX` と同じ「型集合を見ない述語」の一族だが、**別の機構**である:
`allX` は「型集合の全項が述語を満たすか」、`commonUnder` は
「型集合の全項の underlying が同一か、ならそれ」。`Underlying()` を読むと
型パラメータには `TypeParam` が返るので、

```go
func RangeSlice[T interface{ ~[]int }](xs T) int { for range xs { … } }
func Recv[T interface{ ~chan int }](c T) int     { return <-c }
```

が丸ごと ill-typed になっていた。kubernetes の
`apimachinery/pkg/api/validate` の `cannot range over newList` がこれ。

### `mod-year` / `mod-year-range`（goheader）

§4（2026-08-07）に既出。上流は `git log` のコミット日時を優先し、guff は
ファイルの mtime を使う。ファイルごとに git を起動するコストが見合わないため。
**golden fixture ではこの 2 つの値を使わない。**
