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
| うち `both == 0` の空振り合格 | **9 linter**: prealloc, usestdlibvars, maintidx, mirror, musttag, iface, varnamelen, contextcheck, sloglint |
| うち `both == 1` の 1 件だけ比較 | **72 linter** |
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

### Phase 4 — 設定・除外セマンティクスの互換テスト `[未着手]`

現在ほぼゼロの層。ユーザーが実際に踏むのはここ。すべて finding-set を変える ＝ 互換性そのもの。

- 各 linter の settings キーを 有効/無効/閾値/リスト で 3〜4 パターン
- `linters.exclusions.{rules,presets,generated,paths}` / `issues.exclude-rules`
- `issues.uniq-by-line` / `max-issues-per-linter` / `max-same-issues` / `severity.rules`
- `//nolint` の全形（同一行・直前行・`//nolint:a,b`・ブロック・説明付き・不正形式）
- `run.build-tags` / `run.tests` / `run.go` / `output.path-mode`

fixture 1 個 × config N 個の直積で回す。

### Phase 5 — コーパスの多様化 `[未着手]`

現行 8 リポは「普通の Go」に偏っている。踏めていない形:
generics 多用、cgo、build tags、`go.work` マルチモジュール、`vendor/`、`embed`、
テストのみパッケージ、アセンブリ、非 ASCII 識別子、古い go directive、
巨大生成ファイル（protobuf / deepcopy）。

候補: ent（generics + codegen）、tailscale（cgo + tags）、mattermost-server（規模）、
gvisor（unsafe / asm）、kubernetes 全体。

### Phase 6 — 差分ファジングと自動最小化 `[未着手]`

手書き fixture は「思いついた形」しか書けない。

- **まず縮小器 `compat/reduce.py` だけ作る** — 差分が出たら delta-debugging で最小再現に自動縮小し、
  そのまま `compat/golden/` の新 fixture に昇格させる。現在 `hunt.sh` の結果は人間が読む必要があり、
  ここが調査コストのボトルネック。
- その後にミューテーション生成（識別子リネーム、文の並べ替え、括弧付与、型の明示/省略、
  ループ形式変換、nolint 挿入）。

### Phase 7 — 上流ドリフト検知 `[未着手]`

golangci-lint **2.12.2** ピンに対し、週次で最新版と現ピンの両方でゴールデンを再生成して差分を出す。
「上流が変えた」を guff のバグと区別できるようにする。

---

## 3. 進捗表

| Phase | 内容 | コスト | 状態 | 最終更新 |
|:-----:|------|:------:|------|----------|
| 0 | カバレッジ台帳 | 小 | **完了**（設定キー突合は Phase 4 へ移動） | 2026-08-07 |
| 1 | ill-typed / panic / ファイル集合ゲート | 小 | **完了** — 3 つとも CI ゲート化。残件だった goheader 位置つきマッチャも移植済み | 2026-08-07 |
| 2 | `default: all` tier | 小 | **ハーネス完成** — `--all-linters`。差分の解消（recall 数千件）は未着手 | 2026-08-07 |
| 3 | ゴールデン差分の産業化 | 大 | **進行中** — gocritic / goheader / **govet（28 pass）** / **gosec（35 rule）** は ratchet なしで完了。staticcheck 160 check（ratchet: **missing 7** / extra 9）と **revive 99 rule**（ratchet: **missing 1 / extra 3** — 全部「上流の importer 盲目」1 クラスで、§6 のとおり**追従しないと決めた恒久差分**）をゲート化。**stdlib 移植は 5 つとも完了**（SA1000 / SA1001 / SA1002 / SA1007 / SA5009）。**文字列定数をバイト列に**（2026-08-10 5 本目）、**gosec の severity / TryResolve / G602 の再スライス**（2026-08-11） | 2026-08-11 |
| 4 | 設定・除外セマンティクス | 中 | 未着手 | — |
| 5 | コーパス多様化 | 中 | 未着手 | — |
| 6 | 縮小器 → 差分ファジング | 中 | 未着手 | — |
| 7 | 上流ドリフト検知 | 小 | 未着手 | — |

**現在の指標**（`docs/COVERAGE.md` / 2026-08-11）: **547** checks 中 `never` **8** / `unit-only` **3** / `fired` **536（98.0%）**。
（計画策定時: 548 checks・`never` 222 / `unit-only` 120 / `fired` 206）

母数が 548 → 547 に減ったのは、**SA9010 が上流に存在しないチェックだった**ため削除したから（§4 の
2026-08-08 の 2 本目のエントリ）。これで Phase 0 が残していた「staticcheck 161 モジュール」の内訳が確定し、
guff は上流 `honnef.co/go/tools@v0.7.0` の **160 check をちょうど実装している**状態になった。

`unit-only` が 102 → 21 に落ちたのは 2026-08-10 の revive ゴールデン化（83 件）、
21 → 3 に落ちたのは 2026-08-11 の gosec ゴールデン化（18 件）による。
**残る 3 件は revive 2 / golines 1 だけ**で、「撃つことは確認済み・同じものを撃つかは未確認」の
在庫はほぼ尽きた。次の投資先は `unit-only` ではなく、gosec の DEFERRED のように
**そもそも実装が無い check**（§4 の 2026-08-11「次にやること 3」）と、
ratchet が残っている staticcheck / revive の側にある。

`never` の 8 件は staticcheck 3（`S1030` / `SA1027` / `SA3000`）/
govet 2（`cgocall` / `framepointer` — どちらも §6）/ gocritic 1（`whyNoLint`、§6）/
revive 1（`time-naming`）/ swaggo 1。**うち 3 件は §6「恒久的に観測できない」側**なので、
潰せる `never` は実質 5 件しか残っていない。`unit-only` 102 のうち 83 は revive で、こちらは
「撃つことは確認済み・同じものを撃つかは未確認」のまま（Phase 3 の残り）。

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

## 5. 既知の「暗黙 allowlist」台帳

`compat/normalize.py` が消している差分。Phase 3 の golden tier では正規化しないので、
ここに挙げたものは**個別に潰す or 恒久的な非互換として理由付きで記録する**必要がある。

| # | 対象 | 正規化が消しているもの | 状態 |
|---|------|------------------------|------|
| 1 | errcheck | callee 名を含む形 (`Error return value of \`f\` is not checked`) と含まない形 | 未調査 |
| 2 | unused | メッセージ先頭の prefix / メソッド修飾 | 未調査 |
| 3 | staticcheck | `SA1234: ` チェックコードを**両側から**剥がす → コード取り違えが不可視 | 未調査 |
| 4 | staticcheck | QF1011「could omit type」/ ST1023「should omit type」の言い回し | 未調査 |
| 5 | staticcheck | Deprecated 文の末尾ピリオド有無 | 未調査 |
| 6 | modernize | チェック名 prefix | 未調査 |
| 7 | govet | pass 名 prefix / `(declared using go1.X.Y)` のパッチバージョン | 意図的（環境差） |

### 明示的な allowlist（`compat/allowlists/`）

上の表は「正規化が黙って消しているもの」。こちらは**ファイルに書いてある**もの。
`--update-allowlist` はファイルのコメントを消してしまうので、**理由はここが正典**。

| 対象 | 件数 | key | 理由 | 記録日 |
|------|-----:|-----|------|--------|
| consul | 1 | `agent/consul/catalog_endpoint.go:280` SA5011 | 上流 IR の σ ノードによる分岐内の値の絞り込みが guff に無い（§7）。誤検出。 | 2026-08-09 |
| consul | 2 | `agent/event_endpoint_test.go:115` / `agent/http_test.go:1728` SA9008 | 上流の IR 検証（`ValueForExpr` + `irutil.Flatten`）未移植。パターン自体は一致済み。誤検出。§4 の 2026-08-09（2 本目）に最小再現。 | 2026-08-09 |

これ以外の allowlist ファイルは**すべてヘッダのみ（0 件）**。3 件を記録したのは
`oss-nightly` を CI ゲートにするため — 恒久的に赤いゲートは次の劣化に日付を付けられない。
**この 3 件を消すのが Phase 3 の残タスク（次にやること 2 / 3）**であり、
消えたらこの節ごと削ること。

加えて、`issue_key` が **column / severity / SuggestedFix を比較していない**（§1）。
うち **column と severity は golden tier（`compat/golden/`）が比較するようになった**が、
それはゴールデンを持つ check に限る。gocritic では実際に 42 件の column バグが出た（§4）ので、
**残りの linter にも同種のバグがあると考えるのが妥当**。SuggestedFix は依然どこも比較していない。

---

## 6. 恒久的に観測できない check

ゴールデンでも OSS でも原理的に捕まえられないもの。「未着手」ではなく「不可能」として記録する。

| check | 理由 |
|-------|------|
| `gocritic/whyNoLint` | 説明のない `//nolint` を報告する checker だが、その `//nolint` 自身が同じ行の findings を抑止するため、golangci-lint の出力に現れない（上流に食わせても 0 件）。単体テストでのみ検証可能。 |
| `govet/framepointer` | **golangci-lint は `.s` ファイルの診断を 1 件も出さない**。同じ fixture に `go vet` を食わせると framepointer 2 件 + asmdecl 4 件が出るのに、golangci-lint 2.12.2 は 0 件（`GOARCH` を合わせても、ホスト arch のままでも同じ）。**この行の以前の理由（GOARCH がホスト依存だから）は誤り**で、ケース単位の環境変数を入れても解けない — その仕組み自体は 2026-08-11（2 本目）で入れてあり、`SA1027` はそれで回収できた。単体テストでのみ検証可能。 |
| `govet/cgocall` | `import "C"` を含むファイルが要る。cgo と C コンパイラを CI ゲートの前提にしたくない。単体テストでのみ検証可能。 |
| `golines` / `swaggo` | どの corpus リポも有効にしておらず、isolate にも fixture が無い。**isolate の `make_config.py` が `linters.enable` しか書けない**のに対し、golangci-lint v2 でこの 2 つは `formatters:` ブロックの住人なので、fixture を置くだけでは足りない。→ 次にやること。 |

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

波及として、**`buildir` の `SrcFuncs` に既定でメソッドを入れられない**。
上流は常に入れるが、入れた瞬間にこの SA5011 偽陽性がメソッド本体から噴き出して
regress ゲートが落ちる。現状は
`BuildIrResult::src_funcs_with_methods()` で**チェック単位のオプトイン**にしてある
（SA4017 のみ）。**src_funcs を回す他の 20 以上の analyzer はメソッドを見ていない
＝ 静かな recall 損失が残っている。** 解くには SA5011 に σ 相当の手当て
（分岐をまたぐ値の区別）を入れるのが先。

### `MakeInterface` がオペランドを持たない（SA4006）

guff-ssa の `MakeInterface` は **空構造体** (`pub struct MakeInterface {}`) で、
ボクシングされる値を保持しない。go/ssa の `MakeInterface` は `X` を持ち、
その値の referrer になる。したがって

```go
var i interface{} = 1
_ = i
i = n          // 上流は撃たない（n の referrer に MakeInterface がある）
```

で guff は `n` を未使用とみなして SA4006 を撃つ。上流に合わせる分岐は
`sa4006.rs` に置いてあるが、命令がオペランドを持たない以上**発火しえない**。
解くには guff-ssa 側で `MakeInterface { x: Value }` に変えて referrer を
張る必要があり、SSA の構造変更なので単独セッションの範囲に収まらない。
現状の差分は golden の extra 1 件（`sa4006/ok.go`）。

### `mod-year` / `mod-year-range`（goheader）

§4（2026-08-07）に既出。上流は `git log` のコミット日時を優先し、guff は
ファイルの mtime を使う。ファイルごとに git を起動するコストが見合わないため。
**golden fixture ではこの 2 つの値を使わない。**
