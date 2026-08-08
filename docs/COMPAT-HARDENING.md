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
| 3 | ゴールデン差分の産業化 | 大 | **進行中** — gocritic / goheader 完了。staticcheck 160 check をゲート化（ratchet 付き。残差分 missing 49 / extra 36） | 2026-08-08 |
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

### `mod-year` / `mod-year-range`（goheader）

§4（2026-08-07）に既出。上流は `git log` のコミット日時を優先し、guff は
ファイルの mtime を使う。ファイルごとに git を起動するコストが見合わないため。
**golden fixture ではこの 2 つの値を使わない。**
