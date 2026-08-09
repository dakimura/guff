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
| 3 | ゴールデン差分の産業化 | 大 | **進行中** — gocritic / goheader / govet-lostcancel 完了。staticcheck 160 check をゲート化（ratchet 付き。残差分 missing 27 / extra 14）。stdlib 移植は SA1002 / SA1007 完了・SA1000 / SA1001 が残り | 2026-08-09 |
| 4 | 設定・除外セマンティクス | 中 | 未着手 | — |
| 5 | コーパス多様化 | 中 | 未着手 | — |
| 6 | 縮小器 → 差分ファジング | 中 | 未着手 | — |
| 7 | 上流ドリフト検知 | 小 | 未着手 | — |

**現在の指標**（`docs/COVERAGE.md` / 2026-08-08）: **547** checks 中 `never` **23** / `unit-only` 104 / `fired` 420。
（計画策定時: 548 checks・`never` 222 / `unit-only` 120 / `fired` 206）

母数が 548 → 547 に減ったのは、**SA9010 が上流に存在しないチェックだった**ため削除したから（§4 の
2026-08-08 の 2 本目のエントリ）。これで Phase 0 が残していた「staticcheck 161 モジュール」の内訳が確定し、
guff は上流 `honnef.co/go/tools@v0.7.0` の **160 check をちょうど実装している**状態になった。

`never` の 23 件は govet 16 / staticcheck 4（`S1030` / `SA1011` / `SA1027` / `SA3000`）/
gocritic 1（`whyNoLint`、§6）/ revive 1（`time-naming`）/ swaggo 1。
**残りは実質 govet だけ**。`unit-only` 104 のうち 83 は revive で、こちらは
「撃つことは確認済み・同じものを撃つかは未確認」のまま（Phase 3 の残り）。

**この指標だけを見ないこと。** 2026-08-08 の SA4006（教科書どおりの形を 1 件も撃てていなかった）と
2026-08-09 の `uniq-by-line` / SA4017 のベンチ除け（どちらも `fired` 済み check の誤検出）は、
**台帳の数字を 1 も動かさない欠陥**だった。`fired` は「golangci-lint と一度でも突合された」であって
「一致している」ではない。一致の指標は golden の ratchet（現在 missing 27 / extra 14）と
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

---

## 7. アーキテクチャの違いで再現できないもの

§6 が「上流に食わせても観測できない」なら、こちらは「観測はできるが guff の
構造上そのままでは再現できない」。**allowlist ではなく、代償を明記した設計判断**として記録する。

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
