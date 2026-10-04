# Phase 8 — golangci-lint 2.12.2 → 2.14.0 移植台帳

Phase 7 の `compat/drift.py` は「上流のどこが動いたか」を**出力で**教える。
しかし出力の差分は**ゴールデンに書いた形**の分しか見えない。G407 は fixture が無いので
drift に 1 行も出ないが、2.14.0 では既定で撃つ。逆に drift の 1 行（例: SA4006 が 1 件消えた）は、
check 本体ではなく IR の lift から来ていることがある。

なので Phase 8 は出力ではなく**上流のコード差分**から台帳を作り、drift はその検算に使う:

```
diffs/<module>.diff（*.md / go.sum 除外）  →  項目（kind / guff の場所 / status）
drift-report-2.14.0.md                     →  各項目の drift_refs、説明できない残り
```

`already-matches` は「guff のコードを開いて、この行がこう判定している」という根拠があるものだけ。
推測で付けたものは無い（各グループの critic が反証を試み、通らなかったものは `unsure` か `needs-port` に落とした）。

> 前提: pin は 2.12.2、candidate は 2.14.0。読み取り専用で作成（guff は一切編集していない）。
> 各項目の根拠（guff のどの行を読んでそう判定したか）の全文は作成時の JSON にあり、ここでは要約している。
> 上流の diff は付録 A の手順で再生成できる。
> 「guff の場所」は `crates/` を省略して書く。

---

## 1. 概要

**下表の数字は台帳の生の行数**（後述の重複を含む）。

### グループ別
| グループ | needs-port | already-matches | not-applicable | unsure | 計 |
|---|---:|---:|---:|---:|---:|
| golangci-core | 13 | 5 | 3 | 1 | **22** |
| golangci-golinters | 13 | 3 | 0 | 0 | **16** |
| xtools-modernize | 24 | 4 | 2 | 0 | **30** |
| xtools-passes | 11 | 2 | 0 | 1 | **14** |
| xtools-substrate | 8 | 2 | 4 | 0 | **14** |
| staticcheck-checks | 14 | 5 | 3 | 1 | **23** |
| staticcheck-ir | 9 | 5 | 2 | 2 | **18** |
| staticcheck-xtinternal | 1 | 0 | 1 | 0 | **2** |
| revive | 19 | 2 | 0 | 1 | **22** |
| gosec | 8 | 1 | 0 | 0 | **9** |
| gocritic-goconst | 7 | 2 | 0 | 0 | **9** |
| exhaustruct | 11 | 0 | 0 | 0 | **11** |
| exhaustive-sumtype | 8 | 1 | 0 | 1 | **10** |
| iface-canonicalheader | 12 | 2 | 0 | 0 | **14** |
| formatters | 14 | 3 | 0 | 0 | **17** |
| small-a | 10 | 3 | 0 | 0 | **13** |
| small-b | 16 | 6 | 0 | 0 | **22** |
| **合計** | 198 | 46 | 15 | 7 | **266** |

### kind 別
| kind | needs-port | already-matches | not-applicable | unsure | 計 |
|---|---:|---:|---:|---:|---:|
| behavior | 108 | 31 | 11 | 3 | **153** |
| new-option | 20 | 8 | 0 | 0 | **28** |
| new-check | 25 | 0 | 0 | 0 | **25** |
| substrate | 15 | 4 | 2 | 1 | **22** |
| message | 17 | 1 | 0 | 1 | **19** |
| other | 0 | 0 | 2 | 2 | **4** |
| default-change | 4 | 0 | 0 | 0 | **4** |
| removed-check | 3 | 1 | 0 | 0 | **4** |
| config-schema | 3 | 0 | 0 | 0 | **3** |
| position | 3 | 0 | 0 | 0 | **3** |
| renamed | 0 | 1 | 0 | 0 | **1** |
| **合計** | 198 | 46 | 15 | 7 | **266** |

### 重複について（同じ変更が複数のグループに出る）

上流の 1 変更が、golangci-lint のラッパ（`golangci-core` / `golangci-golinters`）と linter モジュール本体の
**両方の diff に現れる**ことがある。台帳ではどちらの行も残した（片方は配線、片方は意味論の担当）。
PR を数えるときは次の組を 1 つと数えること:

| 変更 | 出てくるグループ |
|------|------------------|
| exhaustruct_v5 の追加 / v4 の deprecated 化 | golangci-core ×3、golangci-golinters、exhaustruct ×2 |
| canonicalheader の fork 乗り換えと `exclusions` | golangci-core、golangci-golinters、iface-canonicalheader ×2 |
| dupword `skip-raw-strings` | golangci-core、golangci-golinters、small-a |
| fatcontext `check-loops` / `check-function-literals` | golangci-core、golangci-golinters、small-b（3 つとも already-matches） |
| funcorder `function` / nonamedreturns `allow-unused-named-returns` | golangci-core、golangci-golinters、small-a |
| goconst `exclude-types` / `ignore-map-keys` | golangci-core、golangci-golinters、gocritic-goconst ×2 |
| gomoddirectives `replace-allow-all` / `ignore-forbidden` | golangci-core、golangci-golinters、small-b ×2 |
| gofumpt `extra.*` | golangci-core、formatters |
| iface `unusedmethod` | golangci-core、golangci-golinters、iface-canonicalheader |
| modernize の suite 変更（fmtappendf 削除、旧名警告） | golangci-core、golangci-golinters、xtools-modernize |
| gochecksumtype の Analyzer 直利用（fact） | golangci-golinters、exhaustive-sumtype |
| revive allRules の 3 規則 | golangci-golinters、revive ×3 |
| SA5011 の削除 | staticcheck-checks、staticcheck-ir |
| unused の Unify | staticcheck-checks、staticcheck-ir |
| SA1019 の SelectorName | staticcheck-checks、staticcheck-ir |
| MaybePointerLike（SA6002 / SA5009） | staticcheck-checks ×2、staticcheck-ir |
| stdversion（govet 既定 analyzer 丸ごと欠落） | xtools-passes、xtools-substrate |
| typeutil.Callee の Origin 化 | xtools-substrate ×2、xtools-passes（unusedresult）、staticcheck-xtinternal |
| G407 の既定有効化 | golangci-golinters、gosec |

重複を畳むと、`needs-port` の**独立した作業単位**はおよそ 170 前後になる（生の needs-port 198 から重複約 25〜30 を引いた数）（下の PR 順序はこの単位で切る）。

---

## 2. 基盤（substrate）の変更を先に

linter 単位で移植を始める前に、**多くの linter を同時に動かす土台**を確かめる。
ここを後回しにすると、linter 側の PR が「土台の差を check の中で当て木する」形になる
。当て木は土台を上流に寄せた日に外れて過剰報告になり、しかも別の近似を支えていることがある。

### 2.1 x/tools v0.44 → v0.50（go/ssa と internal）

| 変更 | guff | status | 説明できる drift |
|------|------|--------|------------------|
| `builder.assign` から複合リテラルの in-place 初期化が消え、`x := T{...}` は一時 Alloc → 1 回の Store（位置は左辺の ident） | `guff-ssa` は**既に v0.50 の形**。v0.44 を再現しているのは `guff-style/src/wastedassign.rs` の AST 近似 `composite_lit_rhs` | needs-port（近似の撤去だけ。S） | wastedassign 4 件（307:2 / 318:2 / 329:2 / 339:2）と消える 2 件（329:12 / 339:21）。**gosec の説明できない 2 系統**（G702 g7xx:73:18 の消滅、G124 g124_elided の +3）も同じ lowering 変更と推定 |
| `typeutil.Callee` / `StaticCallee` が `fn.Origin()` を返す | `guff-analysis/src/code.rs` `call_target_object`、`guff-govet/src/govet_util.rs` `static_callee`、`passes/typeindex.rs` は**インスタンス側の ObjectId** のまま | needs-port（M。共有ヘルパなので golden 全体で実験してから） | unusedresult のジェネリック受信者表記（`[int]` → `[T]`、未測定） |
| `typeindex.Calls` が `F[int](...)` を遡る | `used_ident` が既に剥がしている。ただし Calls の照合は Callee の Origin 化に依存 | needs-port（上と同時） | — |
| `const.go nillable` が `unsafe.Pointer` を nil 可能型に | `guff-govet/src/nilness.rs` `nillable_under` に Basic(UnsafePointer) が無い | needs-port（S） | — |
| range-over-func の yield 内ラベルから `_goto: ycont` が外れた | `guff-ssa/src/builder/range_func.rs:184` は v0.44 のまま | needs-port（M） | — |
| `internal/stdlib/manifest.go` に Go 1.27 シンボル | goimports の `stdlib_exports.txt`（GOROOT 由来、internal を含む）に `uuid` / `crypto/mldsa` / `encoding/json/v2` / `bytes.CutLast` が無い | needs-port（M。**上流マニフェストから転記**、手元 Go で再生成しない） | — |
| unified IR export data V5 / indexed v3 | `guff-exportdata` は V0–V2 だけ | needs-port（L。Go 1.27 toolchain で走らせる日まで不要） | — |
| Go 1.27 ジェネリックメソッド（SSA 全体） | `guff-types` が言語機能を拒否する | not-applicable | — |
| SSA 関数名 `F[int, string]` | `instantiate.rs targstr` は既に一致 | already-matches | — |
| `refactor.AddImport` が宣言なしファイルでも挿入 | `refactor.rs add_import` は既に一致 | already-matches | — |

### 2.2 honnef.co/go/tools v0.7.0 → v0.8.1（go/ir）

honnef IR は **SSI（σ ノード）をやめて go/ssa 相当の pruned SSA に寄った**。guff-ssa はもともと go/ssa 移植なので
土台は上流が guff に寄ってきた形だが、**各 check が v0.7 の SSI を模倣するために書いた近似**が今度は過剰になる。

| 変更 | guff | status | 説明できる drift |
|------|------|--------|------------------|
| Sigma / Copy の削除 | guff-ssa に Sigma は無い。ただし `sa4008.rs`（assigned_in）、`sa5011.rs`（sigma_shadows）、`sa4006.rs`（AST ident hybrid）が SSI を再構成している | needs-port（L。check ごとに模倣を撤去） | staticcheck-s `s1017/ok/ok.go:7:3 SA4006` の消滅 |
| DebugRef が非公開になり Referrers から消える | `guff_analysis::referrers` は DebugRef を含み、check が個別に filter。`sa6001.rs:193` は DebugRef の数を判定に使う | needs-port（M。referrer 消費箇所の監査） | staticcheck-sa `sa4006/bad/bad.go:29:6 SA6001` の消滅（guff は既に出していない） |
| 単一 Exit ブロック廃止、noreturn は `Panic("noreturn")` + unreachable | `guff-ssa/src/emit.rs:453` が既に同形 | already-matches | — |
| defer を持つ関数に **Recover ブロック**、`Function.Returns()` に含まれる | `guff-ssa/src/function.rs:215` DEFERRED、`dom.rs` / `blockopt.rs` TODO | needs-port（M。消費者 SA5012 fact / nilness fact が未移植なので今は観測差なし） | — |
| 定数が命令でなくなり、使用ごとに別の Const | `program.rs:338` が既に使用ごとに alloc | already-matches | — |
| `IsStub` が MakeInterface / Alloc を許可、外部関数を stub としない。`IsTrivial` 新設 | `passes/facts/purity.rs:223 is_stub` は v0.7 相当 | needs-port（S） | （st1005 の SA4017 消滅は IsStub では説明できない、§5） |
| buildir が直接 import だけ作る / FuncValue が nil を返す | 推移閉包で作る。消費者は SA4023 の未移植枝だけ | not-applicable（SA4023 の Call 枝を移植するとき再検討） | — |
| nilness fact を DFA に全面書き換え、typedness 削除 | どちらも未移植 | needs-port（L。SA4023 の Call/Extract 枝の前提） | — |
| `IsPointerLike` → `MaybePointerLike` | `callcheck.rs:721` は Interface なら true で既に一致 | already-matches | — |
| `CoreType` が要素型の異なる chan 項で nil | honnef CoreType の直接移植なし | unsure（極端な端例） | — |
| `typeutil.Unify` 追加、unused の implements が完全単一化に | `guff-unused/src/lenient_implements.rs` は旧 methodsChecker | needs-port（M） | unused `genericiface/generic_iface.go:30,31` の extra 2 |
| `SelectorName` の fallback 2 枝 | `code.rs knowledge_selector_name` に無い | needs-port（S） | SA1019 の文言（§4 staticcheck-checks） |
| `EnclosingFunction` が `func _()` を見つけない | SA9008 は AST 実装で `_` を見ない。SA4031 は contextcheck 有効時だけ `_` が入る | needs-port（S） | — |
| ジェネリクス周りを go/ssa に再同期（issue 78110 の拡幅、MethodVal の Source を Sel に） | 土台は同系譜だが 78110 は 0 件 | unsure（M） | — |
| 宣言が Go 1.27 の昇格フィールドキー | guff-types が型検査で拒否 | not-applicable | — |

### 2.3 staticcheck の vendored x/tools internal（versions / typeindex）

- `versions.FileVersion` が版不明のファイルを `Future` にし、`for` を go1.22+ で組む。
  **guff-ssa の `for_stmt` には `forStmtGo122` 相当が無い**（range は 1.22+ で組んでいる）。
  通常ケース（go.mod が 1.22+）でも、クロージャが 3 節 for のループ変数を捕捉する形で honnef IR と alloc の形が違うはず。
  → needs-port（S〜M）。SA4006 / SA4010 / SA5011 系の fixture で測る。
- typeindex の generic Origin 記録 → 現在の Symbol パターンに generic が無いので not-applicable。

### 2.4 formatter の printer（go1.27）と toolchain

gofumpt v0.12 は vendored `go/printer` を go1.27 に更新した（`indentList` の複合リテラル除外、
`intersperseComments` の IDENT 条件、`exprList` の `log2ish` / `exp2ish`、doc link の std 一覧に `uuid`）。
**guff の printer は `guff-ast/src/printer` を gofmt / goimports / gofumpt で共有している**。
gofmt / goimports は golangci-lint バイナリをビルドした toolchain の `go/printer` を使うので、
`go version -m $(which golangci-lint)` で 2.14.0 のビルド Go を確認してから、
「gofumpt だけ go1.27」か「全 formatter を go1.27」かを決める（4 項目とも needs-port / S、ただし切り替えの単位が先）。

**確認済み（2026-10-04）**: `go version -m compat/.tools/golangci-lint-2.14.0/golangci-lint` は **go1.27.0**
（2.12.2 は go1.26.2）。golines v0.15.0 と swaggoswag は同版。したがって gofmt / goimports / swaggo も
go1.27 の `go/printer` で動いている —— 切り替えの単位は「全 formatter を go1.27」。

同じ理由で **`compat/oracles/` の go ディレクティブ**（golangci-lint 自身の `go` 行に揃える規則）も
1.25.0 → **1.26.0** に上がる。これは `urlstrictcolons` の既定を反転させ（`gostd/url.rs` の挙動変更）、
Unicode 表は go1.27 の toolchain で再生成することになる。どちらも guff の挙動変更を伴うので
PR 1（データのみ）からは外し、独立した PR にする。

### 2.5 revive の internal failure

v1.17 は rule の internal failure でファイルの残り rule を捨てなくなった。guff は最初から何も捨てないので already-matches。
上流の「実行ごとに finding が落ちる」非決定性の 1 源が消えるので、`golden.py` の 2 回一致ループのコストが下がるかを 2.14.0 で実測したい。

---

## 3. config 層

linter を 1 本も移植しなくても、**ユーザの `.golangci.yml` を受理するか**と**既定の意味**は pin を上げた日に変わる。
guff の `parse_settings` は未知キーを黙って無視するので、設定しても効かないキーは「エラーにならずに違う結果」になる
（最も見つけにくい側）。ここを linter 移植より先に揃える。

### 3.1 新 linter / inventory

| 項目 | 上流 | guff の現状 | status |
|------|------|-------------|--------|
| `exhaustruct_v5`（Since v2.13.0、analyzer 名・`//nolint` 名とも `exhaustruct_v5`） | `enable` に書ける、`default: all` に入る | `registry.rs` / `KNOWN_LINTER_NAMES` に無い → `cli.rs:353` の unknown linters で **exit 3** | **PR 2**: 名前を受理、v4 エンジンで代用、golden `exhaustruct-v5` に ratchet 3/0 |
| `exhaustruct`（v4）の deprecated 化 | 警告レベル、実行は続く、Migration なし | deprecation 警告の仕組み自体が無い（wsl / gomodguard も同じ） | **done (PR 2)**: exhaustruct / wsl / gomodguard の 3 本。Migration 提案は未 |
| modernize `autoFix: true` | `help linters` の表示だけ | guff の一覧は capability タグを出さない | not-applicable |
| golines / revive の URL | メタデータだけ | — | not-applicable |

**最低限のつなぎ**: `exhaustruct_v5` の名前を受理し（`default: all` の集合にも入れる）、settings を読む。
analyzer 本体（L）が入るまでは finding 0 件になる —— これは「未実装」として ratchet に載せる（黙って 0 件にしない）。

### 3.2 新しい settings キー（上流の既定値つき）

| linter | キー | 上流既定 | guff | status |
|--------|------|----------|------|--------|
| exhaustruct_v5 | enforce-patterns / ignore-patterns / optional-patterns / allow-empty / allow-empty-patterns / allow-empty-returns / allow-empty-declarations / **allow-empty-blank-assignments**（struct のみ、schema に無い） / explicit-mode | false / 空。report-full-type-path は配線されず常に false。不正パターンは Fatalf | 無し | needs-port |
| canonicalheader | `exclusions` []string / `use-default-exclusions` | 空 / **true** | 無し（DEFERRED の理由「YAML 設定が無い」は 2.14.0 で腐った） | needs-port |
| dupword | `skip-raw-strings` | false | 無し | **done (PR 2)** |
| fatcontext | `check-loops` / `check-function-literals` | true / true | 既に読む | already-matches |
| funcorder | `function` | false | 既に読む | already-matches |
| nonamedreturns | `allow-unused-named-returns` | false | 既に読む | already-matches |
| goconst | `exclude-types`（Assignment/Binary/Case/Return/Call/CompositeLit、大小無視、未知値はエラー） / `ignore-map-keys`、`ignore-calls` は deprecated | **[Call]** / false | 無し | needs-port。**ラッパの後付け規則**: ignore-calls:false かつ exclude-types がちょうど [Call] なら空。明示した exclude-types は既定を**置換**（マージしない） |
| gomoddirectives | `replace-allow-all` / `ignore-forbidden`（struct のみ） | false / false | 無し（ignore-forbidden は DEFERRED） | **done (PR 2)**（ignore 既定ディレクトリ検査と、ブロック内の列も） |
| gofumpt | `extra.group-params` / `extra.clothe-returns` / `extra.balance-calls`、`extra-rules` は deprecated | false ×3 | `extra-rules` だけ | needs-port。**文言とコードが食い違う**: 警告は「use extra.group-params instead」だが、`extra-rules: true` は 3 規則すべて（balance-calls 含む）を有効にする。コードに合わせる |
| iface | `enable: [unusedmethod]`、`settings.unusedmethod.exclude` | 既定は identical のみ | 無し | needs-port |
| modernize | `disable` に書ける名前: fmtappendf 削除、waitgroup → waitgroupgo、7 個追加 | 旧名は warn（照合は改名しない） | 任意文字列を受理（受理は一致）、警告なし | **PR 2: 警告は done**（Suite は PR 1） |
| gosec | `config.global` の `nosec-require-rules` / `nosec-require-justification`、代替タグの `#` 正規化 | 無効 | `config.global` 全体が DEFERRED | needs-port |
| revive | `directives: specify-disable-rule`（と既存の specify-disable-reason） | — | directives を読まない | needs-port |
| revive `line-length-limit` | map 引数 `{max, excludes}` | — | 整数だけ（map だと 80 に落ちる） | needs-port |
| revive `identical-switch-branches` | `allow-identical-default` | false | 引数を読まない | needs-port |
| revive `comment-spacings` | 既定 allow-list に `//#nosec` | — | 組み込み既定なし | needs-port |

### 3.3 既定値・inventory 以外の config 層の挙動

- **cache の DefaultDir**: `GOLANGCI_LINT_CACHE` が相対パス／UserCacheDir 不明のとき、2.14.0 の `run` は
  `log.Fatalf("build cache is required, but could not be located: ...")` で exit 1。guff の run は `default_cache_dir()` の Err を握りつぶして続行する。
  `cache status` で `=off` のとき上流は `Dir: off` で成功、guff は Disabled エラー。→ needs-port（S）。`GUFF_CACHE=off` 拡張をどう残すかの決定が先。
- **SA5011 を `staticcheck.checks` に明示した config**: 上流から analyzer が消えた後に無視かエラーか未確認（§7）。
- **revive の未知 rule 名**: 上流 2.12.2 は `cannot find rule` でエラー、guff は無視。2.14.0 で `marshal-receiver` / `use-slices-concat` を名指しした config が guff では黙って 0 件になる。

---

## 4. linter ごとの台帳

凡例: status = `needs-port` / `already-matches` / `not-applicable` / `unsure`。effort = S / M / L。
testdata 列は fixture にする上流ファイル（`—` は無し）。

### golangci-core

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | lintersdb/builder_linter.go `exhaustruct.NewV5` | new-check | 新 linter exhaustruct_v5 | guff-lint/src/registry.rs, cli.rs:336-353 | **PR 2: v4 エンジンで代用**（tag・パターン・explicit-mode は PR 14） | L | exhaustruct_v5.go / _custom.{go,yml} / _cgo.go |
| 2 | config/linters_settings.go ExhaustructV5Settings | config-schema | `linters.settings.exhaustruct_v5` 新設 | guff-lint/src/settings.rs（v4 のみ） | **PR 2: 3 キーのみ配線、残りは警告** | M | — |
| 3 | builder_linter.go exhaustruct DeprecatedWarning | message | v4 の deprecated 警告と `[deprecated]` 表示 | registry.rs format_linters_listing / cli.rs | **done (PR 2)**（`[deprecated]` 表示と Migration 提案は未） | S | — |
| 4 | builder_linter.go modernize WithAutoFix | other | 一覧の `[auto-fix]` 表示だけ。--fix 可否とは無関係 | registry.rs | not-applicable | S | — |
| 5 | builder_linter.go golines / revive WithURL | other | URL はメタデータだけ | — | not-applicable | S | — |
| 6 | CanonicalHeaderSettings + canonicalheader.New | behavior | fork 乗り換え + exclusions / use-default-exclusions | guff-style/src/canonicalheader.rs, settings.rs | needs-port | M | canonicalheader_custom.{go,yml} |
| 7 | DupWordSettings.SkipRawStrings | new-option | dupword `skip-raw-strings` | settings.rs DupwordSettings, guff-comment/src/options.rs | **done (PR 2)** | S | dupword_skip_raw_strings.{go,yml} |
| 8 | FatcontextSettings | new-option | check-loops / check-function-literals（既定 true） | settings.rs:938, guff-context/src/fatcontext.rs:293 | already-matches | S | fatcontext_checkloops.{go,yml}, fatcontext_checkfunctionliterals.{go,yml} |
| 9 | FuncOrderSettings.Function | new-option | funcorder `function` | settings.rs:1363, guff-style/src/funcorder.rs:229,276 | already-matches | S | — |
| 10 | GoConstSettings ExcludeTypes / IgnoreMapKeys | new-option | goconst の新キーとラッパの互換処理 | settings.rs GoconstSettings:502 | needs-port | M | goconst_exclude_types.{go,yml} |
| 11 | GoModDirectivesSettings | new-option | replace-allow-all / ignore-forbidden | settings.rs:2207 | needs-port | S | — |
| 12 | NoNamedReturnsSettings | new-option | allow-unused-named-returns | settings.rs:1342, nonamedreturns.rs:1060 | already-matches | S | — |
| 13 | formatters_settings.go GoFumptExtra | new-option | gofumpt `extra.*`、extra-rules deprecated | guff-lint/src/config.rs:590, guff-fmt/src/gofumpt.rs | needs-port | M | gofumpt_with_extra.{go,yml} |
| 14 | jsonschema iface-analyzers | config-schema | iface `unusedmethod` と exclude | settings.rs IfaceSettings:2137, iface.rs | needs-port | M | iface_unusedmethod.{go,yml} |
| 15 | jsonschema modernize-analyzers | config-schema | disable 名の変更と旧名警告。guff は fmtappendf を既定で走らせ続ける | settings.rs ModernizeSettings, modernize.rs:111,8305 | needs-port | M | — |
| 16 | goformatters/gci standard_list.go | behavior | std 一覧に crypto/mldsa, uuid（guff は go1.26 分の 4 つも欠落、計 6） | guff-fmt/src/native/gci_std_packages.txt | needs-port | S | — |
| 17 | goanalysis/runners.go isOutsideFile | behavior | ファイル外の TextEdit を捨てる（guff は end 側を検査しない） | guff-lint/src/fix.rs:267-315 | needs-port | S | — |
| 18 | runner_action_cache.go inheritFactsFromDeps | substrate | warm cache で facts が欠ける不具合の修正 | guff-runner/src/action.rs, guff-analysis/src/fact_codec.rs:67 | already-matches | S | — |
| 19 | internal/cache position.go / toRelativePath | substrate | issues cache を相対パス化 | guff-runner/src/cache.rs | not-applicable | S | position_test.go, cache_test.go |
| 20 | cache DefaultDir（error を返す） | behavior | 相対パス cache で run が fatal、`cache status` の off | guff-runner/src/cache.rs:77, guff-lint/src/lib.rs:820, cli.rs:1022 | needs-port | S | — |
| 21 | gocheckcompilerdirectives v1.3.0→v1.4.0 | behavior | `//go:fix` を既知に（mods.txt / diffs に無かった） | guff-style/src/gocheckcompilerdirectives.rs:33 | already-matches | S | checkcompilerdirectives/testdata/code.go |
| 22 | go.mod（mods.txt に無い依存） | other | sourcegraph/go-diff v0.9.0（revgrep の extended header 書き直し）と go 1.26 toolchain の影響が未検証 | guff-lint diff.rs（new-from-patch / new-from-rev） | unsure | M | — |

### golangci-golinters

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | golinters/canonicalheader | behavior | fork に差し替え（既定でも文言と報告ノードが変わる）+ 新 option | guff-style/src/canonicalheader.rs | needs-port | M | canonicalheader{,_cgo,_custom}.go, _custom.yml |
| 2 | golinters/dupword | new-option | skip-raw-strings を flag に渡す | guff-comment/src/dupword.rs | **done (PR 2)** | S | dupword_skip_raw_strings.{go,yml} |
| 3 | golinters/exhaustruct/exhaustruct_v5.go | new-check | 新 linter（v5.2.0） | guff-style/src/exhaustruct.rs, registry.rs | **PR 2: v4 エンジンで代用**（tag・パターン・explicit-mode は PR 14） | L | exhaustruct_v5*.go/yml, exhaustruct_v4*.go/yml |
| 4 | golinters/fatcontext | new-option | 2 フラグを渡す | settings.rs, fatcontext.rs | already-matches | S | fatcontext*.go/yml |
| 5 | golinters/funcorder | new-option | function を渡す | funcorder.rs:276 | already-matches | S | — |
| 6 | golinters/gochecksumtype | substrate | Analyzer 直利用、sumTypeFact で依存パッケージの sum type を検査 | guff-style/src/gochecksumtype.rs | needs-port | M | — |
| 7 | golinters/goconst runGoconst/toType | new-option | exclude-types の意味論（ignore-calls を変えても call が除外されないケース） | settings.rs, guff-style/src/goconst.rs:272 | needs-port | M | goconst_exclude_types.{go,yml}, goconst_eval_and_find_duplicates.go |
| 8 | golinters/gomoddirectives | new-option | replace-allow-all / ignore-forbidden を渡す | guff-import/src/gomoddirectives.rs, options.rs:66 | **done (PR 2)** | S | — |
| 9 | golinters/gosec New | new-check | `Excludes += "G407"` の暫定処理を削除 → G407 が既定で走る | guff-style/src/gosec.rs | needs-port | L | — |
| 10 | golinters/iface | new-check | enable に unusedmethod | iface.rs | needs-port | M | iface_unusedmethod.{go,yml} |
| 11 | golinters/internal/util.go FormatCode | message | `%#q` 化（CanBackquote 偽ならダブルクォート）。goconst / errcheck / dupl / gocyclo / gocognit / gochecknoinits | goconst.rs:330, gocyclo.rs:39, gocognit.rs:40, guff-errcheck/src/lib.rs:210, guff-dupl/src/dupl.rs:59 | needs-port | S | — |
| 12 | golinters/misspell | message | `%#q` 化 | guff-misspell/src/misspell.rs:30 | needs-port | S | — |
| 13 | golinters/nolintlint/internal/issues.go | message | `%#q` 化 | guff-lint/src/nolintlint.rs:206, nolint.rs:603 | needs-port | S | — |
| 14 | golinters/modernize | message | fmtappendf / waitgroup の旧名警告 | settings.rs, modernize.rs | needs-port | S | fix/in,out の waitgroupgo / waitgroup / fmtappendf |
| 15 | golinters/nonamedreturns | new-option | allow-unused-named-returns を渡す | settings.rs:1347, nonamedreturns.rs:1060 | already-matches | S | — |
| 16 | golinters/revive | behavior | revive v1.17 の公開 API に寄せ、enable-all に 3 規則が増える | guff-revive/src/config.rs AHEAD_OF_PIN_RULES:148 | needs-port | M | revive.yml |

`%#q` の 3 項目は `guff_gostd::strconv::quote_sharp` / `can_backquote`（guff-gostd/src/strconv.rs:54,69）を共有すれば S で済む。

### xtools-modernize

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | modernize.go Suite / fmtappendf.go | default-change | fmtappendf が Suite から外れた | settings.rs SUITE_EXTRA_OFF, modernize.rs | **done (PR 1)** | S | — |
| 2 | Suite / errorsastype.go | default-change | errorsastype が既定有効 | SUITE_EXTRA_OFF, check_errorsastype | **done (PR 1)** | S | errorsastype.go{,.golden} |
| 3 | errorsastype.go canUseErrorsAsType | behavior | 否定形・`_`・パッケージ var / 非 error 型の除外 | check_errorsastype | already-matches | S | errorsastype.go{,.golden} |
| 4 | importcomment.go | new-check | canonical import comment（module モード） | check_importcomment | PR 1 で Suite 入り。golden の形は一致 | S | — |
| 5 | reflecttypeassert.go | new-check | `v.Interface().(T)` → `reflect.TypeAssert[T]` | check_reflecttypeassert | PR 1 で Suite 入り。golden の形は一致 | S | reflecttypeassert.go{,.golden} |
| 6 | embedlit.go | new-check | 埋め込みフィールド型の省略（Go 1.27+） | 無し | needs-port | L | embedlit*.go{,.golden} |
| 7 | slicesclip.go | new-check | `x[:len(x):len(x)]` → slices.Clip | 無し | needs-port | S | slicesclip.go{,.golden} |
| 8 | reflect.go usesNonTypeSymbol | behavior | 非型シンボルを含む TypeOf を報告しない、DeleteUnusedVars 削除 | check_reflecttypefor, delete_newly_unused_vars | **done (PR 1)** | M | reflecttypefor.go{,.golden} |
| 9 | slicescontains.go | behavior | needle / predicate に副作用がありうれば報告しない | slicescontains_cond, expr_may_have_effects | **done (PR 1)** | S | slicescontains.go{,.golden} |
| 10 | stringsbuilder.go | behavior | _test.go を報告しない | check_stringsbuilder | **done (PR 1)** | S | stringsbuilder{,_test}.go{,.golden} |
| 11 | stringscut.go stringsplitCut | new-check | `strings.Split(s, sep)[0]` → Cut | check_stringscut | needs-port | M | stringscut.go{,.golden} |
| 12 | stringscut.go（LastIndex） | behavior | LastIndex 系 → Contains / CutLast（1.27） | check_stringscut FUNCS | needs-port | M | stringscut{,_go127}.go{,.golden} |
| 13 | stringscut.go 多値代入 | behavior | 多値の宣言・代入の一部なら報告しない | cut_i_ident | needs-port | S | stringscut.go{,.golden} |
| 14 | stringscut.go indexArgValid | behavior | IsAssignedOrAddressTaken で全使用を見る | cut_has_modifying_uses | needs-port | S | stringscut.go{,.golden} |
| 15 | minmax.go if パターン | behavior | 比較式に副作用があれば報告しない | check_minmax, check_minmax_block | needs-port | S | minmax/parametersideeffect/* |
| 16 | minmax.go checkUserDefinedMinMax | behavior | 比較の両辺が引数名と一致することを要求 | check_user_defined_minmax | needs-port | S | minmax/userdefined/** |
| 17 | atomictypes.go unkeyedFields | behavior | キー無しリテラルで使われる struct のフィールドを除外 | check_atomictypes | needs-port | S | atomic.go{,.golden} |
| 18 | waitgroupgo.go cannotRecover | behavior | 本体の defer が recover しうれば末尾 Done 形を報告しない | waitgroupgo_done_span | needs-port | S | waitgroup.go{,.golden} |
| 19 | testingcontext.go | behavior | 先行する defer があれば報告しない、fix の分割 | check_testingcontext_list | needs-port | S | testingcontext_test.go{,.golden} |
| 20 | rangeint.go IsAssignedOrAddressTaken | behavior | ポインタレシーバのメソッド呼び出しを lvalue に | collect_scalar_lvalues | needs-port | S | rangeint.go{,.golden} |
| 21 | slicesbackward.go init.Tok | behavior | `i = len(s)-1`（ASSIGN）を報告しない | check_slicesbackward | needs-port | S | slicesbackward.go{,.golden} |
| 22 | slicesbackward.go s[i] lvalue | behavior | index 位置に i がある**任意の** IndexExpr が lvalue なら報告しない（X==s に限定しないこと） | index_mutated_in_body | needs-port | M | slicesbackward.go{,.golden} |
| 23 | slicesbackward.go fix | behavior | 値変数名の選択と `name := s[i]` の削除（字句順で最初、先頭文でなくてよい） | check_slicesbackward（"v" 固定） | needs-port | M | slicesbackward.go.golden |
| 24 | stringscutprefix.go | behavior | pattern 2 の未使用 after を `_` に（fix のみ） | check_stringscutprefix | needs-port | S | stringscutprefix.go{,.golden} |
| 25 | sortslice.go | behavior | 引数 2 個以外・副作用ありの s を除外 | check_slicessort | already-matches | S | slicessort.go{,.golden} |
| 26 | newexpr.go | behavior | 可変長関数を new-like としない | newexpr_signature_shape_ok | already-matches | S | newexpr.go{,.golden} |
| 27 | stringsseq.go | behavior | 変換式で panic していた件 | split_or_fields_seq_name | already-matches | S | splitseq/conv/nobytes.go |
| 28 | bloop.go | behavior | レシーバ一致時だけ削除 | （Suite 外） | not-applicable | S | bloop_test.go{,.golden} |
| 29 | slices.go appendclipped | behavior | Clone を外して畳む | （Suite 外） | not-applicable | S | appendclipped.go{,.golden} |
| 30 | stringsbuilder.go lastEditEnd | behavior | 重複回避の境界が前にずれ、2 変数交互の形で 2.14 が報告する | check_stringsbuilder（max を使う＝旧挙動） | needs-port | S | （upstream に直接の case 無し、自作） |

### xtools-passes（govet）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | fieldalignment.go | message | 文言を全面変更（型名、size class、waste） | guff-govet/src/fieldalignment.rs check_struct | needs-port | M | a.go, a_amd64.go, a_386.go |
| 2 | composite.go | behavior | 型パラメータ複合リテラルを先頭 term だけで判定 | guff-govet/src/composites.rs | needs-port | S | a.go{,.golden}, flag/flag.go |
| 3 | inline.go withinTestOf | behavior | 専用テスト内の使用を抑制（同ディレクトリ、Fuzz、const / alias） | guff-govet/src/inline.rs | needs-port | M | issue76190.txtar |
| 4 | inline.go 埋め込みフィールド判定 | behavior | `Defs[id].Embedded()` に変更 | inline.rs | needs-port | S | issue78994.txtar |
| 5 | printf.go okPrintfArg | behavior | go1.27+ で %d からポインタを外す | guff-govet/src/printf.rs verb_arg_type | needs-port | M | issue62595/a_go126.go, a_go127.go, a/a.go |
| 6 | printf types.go reason | message | 「(use %p for a pointer)」 | printf.rs match_arg_type | needs-port | S | issue62595/a_go127.go |
| 7 | printf types.go UnsafePointer | behavior | unsafe.Pointer は argPointer の verb だけ受理 | printf.rs | needs-port | S | — |
| 8 | printf.go checkPrint | removed-check | Println の redundant newline 削除 | （元から無い） | already-matches | S | a/a.go |
| 9 | printf.go recursiveStringer Origin | behavior | ジェネリック受信者で再帰検出（親機能ごと未移植の既存ギャップ） | 無し | needs-port | S | — |
| 10 | nilness.go hasCgoUnsafeArgs | behavior | `//go:cgo_unsafe_args` 関数を対象外 | guff-govet/src/nilness.rs | needs-port | S | a/a.go |
| 11 | unusedresult.go inBenchmarkLoop | behavior | `for b.Loop()` 直下を報告しない | guff-govet/src/unusedresult.rs | needs-port | S | a/a.go |
| 12 | unusedresult.go（Callee Origin） | message | ジェネリック受信者表記 `[int]` → `[T]` | unusedresult.rs callee_obj | unsure | S | typeparams/typeparams.go |
| 13 | hostport.go | behavior | 引数 2 未満で panic | hostport.rs:184 | already-matches | S | a/a.go{,.golden} |
| 14 | stdversion.go | behavior | 文言変更 + 除外移動。**guff に analyzer が丸ごと無い**（govet 既定有効） | 無し（settings.rs:3091 に名前だけ） | needs-port | L | stdversion/testdata/test.txtar |

### xtools-substrate

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | go/ssa/builder.go assign | substrate | in-place compLit 経路の削除 | guff-style/src/wastedassign.rs composite_lit_rhs | needs-port | S | ssa/testdata/objlookup.go, valueforexpr.go |
| 2 | go/ssa/const.go nillable | behavior | unsafe.Pointer を nil 可能型に | guff-govet/src/nilness.rs:935 | needs-port | S | const_test.go |
| 3 | go/ssa buildYieldFunc lblock | behavior | range-over-func のラベル goto が親へ抜ける | guff-ssa/src/builder/range_func.rs:184 | needs-port | M | — |
| 4 | typeutil/callee.go Callee Origin | behavior | Callee が Origin を返す | code.rs:756, govet_util.rs:132, typeindex.rs:231 | needs-port | M | callee_test.go |
| 5 | typeindex.go Calls | behavior | 明示インスタンス化を遡る + Callee Origin で照合 | typeindex.rs:231-262 | needs-port | S | — |
| 6 | internal/stdlib/manifest.go | substrate | Go 1.27 シンボル（goimports 候補） | guff-fmt/src/native/goimports/stdlib_exports.txt | needs-port | M | — |
| 7 | typesinternal/toonew.go | behavior | stdversion の除外と最短パス | 無し | needs-port | M | — |
| 8 | gcimporter / pkgbits V5 | substrate | export data V5 | guff-exportdata/src/pkgbits/version.rs | needs-port | L | — |
| 9 | go/ssa ジェネリックメソッド一式 | substrate | Go 1.27 generic methods | guff-types/src/signature_check.rs:76 | not-applicable | L | generic_methods_test.go ほか |
| 10 | go/ssa instantiate 名前 | renamed | `F[int, string]`、受信側型引数を名前に入れない | guff-ssa/src/instantiate.rs:29 | already-matches | S | — |
| 11 | internal/refactor/imports.go | behavior | 宣言なしファイルへの AddImport | guff-analysis/src/refactor.rs:120 | already-matches | S | — |
| 12 | internal/refactor/inline/* | behavior | 関数インライナの変更 | （インライナ無し） | not-applicable | L | — |
| 13 | go/packages golist.go / packages.go | behavior | 重複 ID と export data 失敗の扱い | 無し | not-applicable | S | — |
| 14 | refactor/satisfy/find.go | behavior | NormalTerms 走査、ill-typed で panic しない（printf から到達） | printf_wrappers.rs:49 DEFERRED | not-applicable | M | — |

### staticcheck-checks（honnef v0.7.0 → v0.8.1）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | sa1019.go + code.SelectorName | message | 名前が完全修飾名に（`example.com/old.Legacy`、`(*pkg.T).M`、引用符なし import path） | guff-staticcheck/src/sa1019.rs, guff-analysis/src/code.rs | needs-port | M | go1.8/CheckDeprecated/CheckDeprecated.go |
| 2 | sa1019.go checkIdentObj | behavior | composite literal キーの検査と、X が型のとき位置を選択名に | sa1019.rs struct_lit_diagnostics / selector_diagnostic | needs-port | S | go1.0/CheckDeprecated{,.assist_external}/* |
| 3 | s1005.go | removed-check | map comma-ok の `x, _ = m[k]` を報告しない | guff-staticcheck/src/s1005.rs:35 | **done (PR 1)** | S | LintBlankOK.go{,.golden} |
| 4 | sa4003.go | behavior | 型名を tx そのものに、型パラメータを type set で | sa4003.rs | needs-port | M | go1.18/CheckExtremeComparison.go |
| 5 | sa4006.go IncDec / 複合代入 | new-check | `n++` と結果が読まれない複合代入 | sa4006.rs:647 | needs-port | M | CheckUnreadVariableValues.go |
| 6 | sa4006.go hasUse Sigma 削除 | substrate | sigma 越しの未使用が消える | sa4006.rs has_use_rec | already-matches | S | — |
| 7 | analysis.go / sa5011.go | removed-check | SA5011 削除 | guff-staticcheck/src/lib.rs:164,329 | needs-port | S | — |
| 8 | sa6005.go | message | `!strings.EqualFold`、fix タイトル小文字 | sa6005.rs:70,83 | needs-port | S | CheckToLowerToUpperComparison.go{,.golden} |
| 9 | sa9010.go（新規） | new-check | `deferred return function not called` | 無し | needs-port | S | sa9010/testdata/** |
| 10 | sa1026.go | behavior | MarshalIndent を追加 | sa1026.rs:139 | needs-port | S | CheckUnsupportedMarshal.go |
| 11 | sa9005.go | behavior | MarshalIndent を追加 | sa9005.rs:146,148 | already-matches | S | CheckNoopMarshal.go |
| 12 | sa5007.go | substrate | exit ブロックの判定（**両版で非報告。guff の既存の過剰報告**） | sa5007.rs, guff-analysis/src/ssa_util.rs:322 | needs-port | S | CheckInfiniteRecursion.go |
| 13 | sa5008/jsonv2.go | behavior | `embed` を有効オプションに | sa5008_json.rs:224 | needs-port | S | — |
| 14 | sa1030.go | message | `%q` に rune(val) | sa1030.rs | needs-port | S | — |
| 15 | unused/implements.go Unify | behavior | 完全単一化 | guff-unused/src/lenient_implements.rs | needs-port | M | generic-interfaces.go |
| 16 | unused.go 昇格フィールドキー | behavior | 途中の埋め込みフィールドを used に | guff-unused/src/lib.rs:590 | needs-port | S | — |
| 17 | sa4010.go | behavior | Sigma / DebugRef を外す | sa4010.rs | unsure | M | CheckIneffectiveAppend.go |
| 18 | sa6001.go | behavior | MapLookup が 1 つ以上必要 | sa6001.rs | already-matches | S | — |
| 19 | sa6002.go MaybePointerLike | behavior | term の無い type-set 制約（comparable 等）を pointer-like に | guff-analysis/src/callcheck.rs:721 | already-matches | S | — |
| 20 | sa5009.go MaybePointerLike | behavior | 同上 | sa5009.rs（型照合なし） | not-applicable | S | — |
| 21 | sa4023.go | behavior | nilness Outer/Inner、IsTrivial | sa4023.rs（Call 枝なし） | not-applicable | L | CheckTypedNilInterface*.go, 28241.go |
| 22 | sa5012.go | behavior | 全 Return を支配 | sa5012.rs（簡略移植） | not-applicable | M | — |
| 23 | sa4008.go | behavior | Phi なら常に非報告（2.14 は報告が増える） | sa4008.rs | already-matches | S | — |

### staticcheck-ir

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | go/ir lift / ssa.go（Sigma 削除） | substrate | SSI 廃止 | sa4006.rs, sa4008.rs, sa4031.rs, sa5011.rs | needs-port | L | irutil/testdata/switches.{go,txtar} |
| 2 | go/ir func.go / source.go（debugRef 非公開） | substrate | Referrers から DebugRef が消える | guff-analysis/src/ssa_util.rs:61, buildir.rs:247, sa6001.rs:193 ほか | needs-port | M | testdata/valueforexpr.go |
| 3 | go/ir exit 廃止 / noreturn | substrate | Exit 廃止（Recover 部分は #17） | guff-ssa/src/emit.rs:453 | already-matches | S | — |
| 4 | go/ir const.go | substrate | 定数が命令でなく、使用ごと | guff-ssa/src/program.rs:330 | already-matches | S | — |
| 5 | irutil/stub.go | behavior | IsStub の拡張、IsTrivial | passes/facts/purity.rs:223 | needs-port | S | — |
| 6 | buildir.go 直接 import | behavior | FuncValue が nil | guff-ssa/src/ssautil/load.rs:310 | not-applicable | S | testdata/indirect.txtar |
| 7 | analysis/facts/nilness（DFA） | substrate | nilness fact 書き換え、typedness 削除 | 無し | needs-port | L | Nilness/*.go |
| 8 | typeutil MaybePointerLike | behavior | 項なし型パラメータ | callcheck.rs:721 | already-matches | S | — |
| 9 | typeutil CoreType | behavior | 異なる chan 項 | 無し | unsure | S | — |
| 10 | typeutil Unify | substrate | 型単一化 | lenient_implements.rs | needs-port | M | — |
| 11 | code.SelectorName | behavior | fallback 2 枝 | code.rs:1042 | needs-port | S | — |
| 12 | knowledge/arg.go MarshalIndent | behavior | 引数位置表 | sa9005.rs:146 | already-matches | S | — |
| 13 | knowledge/deprecated.go | behavior | `(crypto/tls.Config).Rand` | stdlib_deprecations.rs:205 | already-matches | S | — |
| 14 | generate.go（sa5011 無効化） | removed-check | SA5011 が Analyzers から消える | lib.rs:329 | needs-port | S | — |
| 15 | go/ir compLit 昇格キー | behavior | Go 1.27 | guff-types/src/literals.rs:272 | not-applicable | M | — |
| 16 | go/ir generics 再同期 | substrate | 78110 拡幅、MethodVal の Source | guff-ssa/src/instantiate.rs ほか | unsure | M | fixedbugs/issue78110.go ほか |
| 17 | go/ir source.go EnclosingFunction | behavior | `func _()` を見つけない（SA4031 / SA9008） | sa9008.rs, sa4031.rs:150, buildir.rs:153,178 | needs-port | S | — |
| 18 | go/ir createRecoverBlock / dom.go | substrate | Recover ブロック（Returns() に含まれる） | guff-ssa/src/function.rs:215, dom.rs, blockopt.rs:50 | needs-port | M | — |

### staticcheck-xtinternal

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | internal/xtools-internal/versions | behavior | FileVersion の Future 化。**guff-ssa に forStmtGo122 が無い** | guff-ssa/src/builder/stmt.rs:1280 for_stmt | needs-port | S | — |
| 2 | xtools-internal typeindex | behavior | Origin 記録と Calls 拡張（現行パターンでは観測不能） | typeindex.rs:86 DEFERRED | not-applicable | S | — |

### revive（v1.15 → v1.17）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | rule/empty_block.go | behavior | bare な `for range x {}` を報告しない | guff-revive/src/rules/empty_block.rs check_range | needs-port | S | empty_block.go |
| 2 | lint/file.go internal failure | behavior | 失敗した rule だけ読み飛ばす | rules/mod.rs run_enabled_rules | already-matches | S | — |
| 3 | lint/file.go handleConfig | behavior | `disable-line` が範囲を閉じない | guff-revive/src/directives.rs | needs-port | S | revive_disable_directives*.go |
| 4 | lint/file.go specify-disable-rule | new-option | 名前なし disable を報告 | directives.rs | needs-port | M | revive_disable_directives_specify_*.go |
| 5 | rule/marshal_receiver.go | new-check | 新 rule | 無し | needs-port | S | marshal_receiver.go |
| 6 | rule/use_slices_concat.go | new-check | 新 rule | 無し | needs-port | M | use_slices_concat.go, go1.22/use_slices_concat.go |
| 7 | rule/multiline_if_init.go | new-check | allRules 入り | config.rs AHEAD_OF_PIN_RULES | needs-port | S | multiline_if_init.go |
| 8 | rule/redundant_build_tag.go | behavior | `//go:build go1.X` の冗長検出（rule ごと未実装） | 無し | needs-port | M | go1.21/redundant_build_tag*.go, redundant_build_tag.go |
| 9 | rule/use_waitgroup_go.go | behavior | ループ内の go 文も（rule ごと未実装） | 無し | needs-port | M | go1.25/use_waitgroup_go.go |
| 10 | rule/redundant_test_main_exit.go | behavior | m.Run() 由来の Exit だけ報告 | rules/redundant_test_main_exit.rs | needs-port | M | redundant_test_main_exit_test.go |
| 11 | rule/unexported_return.go | behavior | interface 例外を削除 | rules/unexported_return.rs | needs-port | S | unexported_return_package_*.go |
| 12 | rule/comment_spacings.go | default-change | 既定 allow-list に `//#nosec` | rules/comment_spacings.rs | needs-port | S | comment_spacings*.go |
| 13 | rule/deep_exit.go | message | flag.Parse の文言に案内を追加 | rules/deep_exit.rs:50 | needs-port | S | deep_exit.go |
| 14 | rule/enforce_slice_style.go | message | 親ノードで文言を分ける | rules/enforce_slice_style.rs:43,87 | needs-port | S | enforce_slice_style_nil.go |
| 15 | rule/identical_switch_branches.go | new-option | allow-identical-default | rules/identical_switch_branches.rs | needs-port | S | identical_switch_branches_allow_identical_default.go |
| 16 | rule/line_length_limit.go | new-option | map 引数と excludes | rules/line_length_limit.rs | needs-port | M | line_length_limit{,_excludes}.go |
| 17 | rule/package_comments.go | behavior | CRLF での終端行計算（判定も動く） | rules/package_comments.rs | needs-port | S | package_comments/issue607_* |
| 18 | rule/redefines_builtin_id.go | behavior | `comparable` を追加 | rules/redefines_builtin_id.rs | needs-port | S | redefines_builtin_id.go |
| 19 | rule/use_any.go | behavior | Go 1.18 未満は黙る | rules/use_any.rs | needs-port | S | use_any.go, go1.18/use_any.go |
| 20 | rule/use_errors_new.go | behavior | Go 1.26 以上は黙る | rules/use_errors_new.rs | needs-port | S | go1.26/use_errors_new.go |
| 21 | rule/add_constant.go | behavior | 空文字列リテラルを報告しない | rules/add_constant.rs | already-matches | S | add_constant_default.go |
| 22 | rule/unhandled_error.go | behavior | `fmt.Fprintf(&buf, ...)` の例外。**ホスト依存**（importer の GOROOT が正しいホストでは 2.14 で変わる） | rules/unhandled_error.rs callee_is_local | unsure | S | unhandled_error.go |

revive の upstream testdata は module zip に無く、手元 checkout（v1.15.0-59）も v1.17.0 ではない。fixture 化には v1.17.0 タグの fetch が要る。

### gosec（v2.26.1 → v2.29.0）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | golinters/gosec + analyzers/hardcoded_nonce.go | new-check | G407 が既定で走る（878 行の SSA analyzer） | guff-style/src/gosec.rs（G407 無し） | needs-port | L | testutils/g407_samples.go |
| 2 | analyzers/context_propagation.go | behavior | G118: IndexAddr / MapUpdate への Store を責任移譲に | guff-style/src/gosec_g118.rs:808 | needs-port | S | g118_samples.go |
| 3 | analyzers/range_analyzer.go | behavior | G115: min/max の set フラグを OR に | gosec_g115.rs:1628 | needs-port | S | g115_samples.go |
| 4 | analyzers/pathtraversal.go | behavior | G703 のサニタイザから Clean / Abs / PathEscape を削除 | gosec_taint.rs:293 | needs-port | S | g703_samples.go |
| 5 | rules/rand.go | behavior | G404 に Perm / Shuffle / ExpFloat64 / v2.Uint | gosec.rs:157 | needs-port | S | g404_samples.go |
| 6 | resolve.go TryResolve（Builder） | behavior | 定数だけで組んだ Builder の String() を定数扱い。**`try_resolve` と `g202_try_resolve` の両方に** | gosec.rs:3130, :1020 | needs-port | M | g202_samples.go |
| 7 | analyzer.go / config.go nosec-require-* | new-option | global オプション | gosec.rs:2148, settings.rs:1906 | needs-port | M | — |
| 8 | config.go NoSecTag | behavior | 代替タグの `#` 正規化 | gosec.rs:2336 | needs-port | S | — |
| 9 | rules/hardcoded_credentials.go | behavior | G101 に ASIA | gosec.rs:369 | already-matches | S | g101_samples.go |

### gocritic-goconst

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | gocritic rules.go sprintfQuotedString | new-check | `` "`%s`" `` → `%#q` の 2 本目の規則 | guff-style/src/gocritic.rs:7166 | needs-port | S | sprintfQuotedString/{positive,negative}_tests.go |
| 2 | gocritic utils.go goStdlib | message | importShadow の stdlib 表 | gocritic.rs:7623 | already-matches | S | — |
| 3 | goconst api.go スコープ分割 | behavior | テスト / 非テストで件数を分ける | goconst.rs run() | needs-port | S | api_test.go |
| 4 | goconst api.go sortPositions | position | ファイル内の最小位置で報告 | goconst.rs first_per_file | needs-port | S | — |
| 5 | goconst match-constant | behavior | 非テスト issue に非テスト const だけ | goconst.rs find_matching_const | needs-port | S | — |
| 6 | goconst find-duplicates | behavior | スコープ別、ソート | report_duplicate_consts | already-matches | S | — |
| 7 | goconst eval-const-expressions | behavior | Defs ベース、名前位置、valueKey（オプションごと未実装） | goconst.rs（DEFERRED） | needs-port | L | goconst_eval_and_find_duplicates.go, api_test.go |
| 8 | goconst ignore-map-keys | new-option | map のキーを数えない | settings.rs, options.rs, goconst.rs collect() | needs-port | M | visitor_test.go |
| 9 | golangci goconst exclude-types | new-option | exclude-types（ラッパ規則込み） | settings.rs to_guff_goconst, goconst.rs | needs-port | M | goconst_exclude_types.{go,yml}, api_test.go |

### exhaustruct（v4 → v5.2.0）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | golinters/exhaustruct/exhaustruct_v5.go | new-check | 新 linter（WithVersion(5)、`//nolint:exhaustruct` では抑止されない） | 無し | **PR 2: v4 エンジンで代用**（tag・パターン・explicit-mode は PR 14） | L | exhaustruct_v5*.go/yml |
| 2 | builder_linter.go DeprecatedWarning | message | v4 の deprecated 警告 | 無し | **done (PR 2)** | S | exhaustruct_v4*.go/yml |
| 3 | v5 processor.go（tag 廃止） | behavior | `exhaustruct:"optional"` tag を見ず、コメントディレクティブだけ | exhaustruct.rs has_optional_tag | needs-port | M | internal/structure/testdata/*.go |
| 4 | v5 tag-migration-visitor.go | new-check | tag を「not supported anymore」で報告 + fix | 無し | needs-port | M | — |
| 5 | v5 internal/directive | new-check | ディレクティブスキャナと解析エラー 6 種 | 無し | needs-port | L | internal/directive/testdata/*.go |
| 6 | v5 shouldCheck / isFieldRequired | behavior | 優先順位（使用箇所 > 型 > explicit-mode） | exhaustruct.rs check_lit | needs-port | L | structs.go |
| 7 | v5 internal/pattern | new-option | enforce / optional パターン、`Type#Field`、leftmost-longest | exhaustruct.rs compile_patterns / match_full | needs-port | M | — |
| 8 | v5 checkEmptyAllowed | new-option | allow-empty-blank-assignments、括弧越し | exhaustruct.rs empty_struct_allowed | needs-port | S | — |
| 9 | v5 resolveLiteralType | behavior | 別名の表示、`[]*T{{}}`、`type PT *T`、型パラメータ | exhaustruct.rs get_struct_type | needs-port | M | origins.go |
| 10 | v5 struct.go SkippedFields | behavior | `_` を報告しない、埋め込み展開、Go 1.27 昇格キー | exhaustruct.rs skipped_fields | needs-port | L | structs.go |
| 11 | v5 astutil/file-parser.go | substrate | 依存パッケージのソースからディレクティブを読む | 無し | needs-port | M | internal/astutil/testdata/*.go |

v5 の `analyzer/testdata` は module zip に無い。GitHub の v5.2.0 タグから取る。

### exhaustive-sumtype

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | exhaustive comment.go parseDirectives | behavior | ディレクティブを厳密一致に | guff-style/src/exhaustive.rs user_directives / has_comment_prefix | needs-port | S | general/x/directive.go, default-case-required/** |
| 2 | exhaustive switch.go makeInvalidDirectiveDiagnostic | new-check | 「failed to parse directives」（switch / map で発火条件が非対称） | exhaustive.rs run | needs-port | M | enforce-comment/*.go, default-*/** |
| 3 | exhaustive enum.go hasIgnoreDecl | behavior | 宣言側 `//exhaustive:ignore`（alias 経由の定数は外れない） | exhaustive.rs find_enums | needs-port | L | enum/enum.go |
| 4 | exhaustive enum.go 宣言 doc の不正ディレクティブ | new-check | doc 先頭に診断 | find_enums | needs-port | M | enum/enum.go |
| 5 | exhaustive common.go fromType Unalias | behavior | alias 型の switch / map key を検査 | exhaustive.rs enum_for_tag | needs-port | S | — |
| 6 | exhaustive possibleEnumMember Unalias | behavior | alias 型で宣言した定数 | named_type_name / map_finding | already-matches | S | — |
| 7 | gochecksumtype analyzer.go fact | behavior | 直接 import の sum type を fact で | guff-style/src/gochecksumtype.rs | needs-port | L | multiple_sumtypes{,_user}/*.go |
| 8 | gochecksumtype def.go | behavior | alias を variant にしない | gochecksumtype.rs build_def | needs-port | S | with_alias/main.go |
| 9 | gochecksumtype analyzer error（notFoundError） | behavior | TypeSpec の無い `//sumtype:decl` で **goanalysis_metalinter 全体**が失敗 | gochecksumtype.rs find_sumtype_decls | needs-port | M | — |
| 10 | gochecksumtype check.go（位置と重複） | other | Diagnostic 経由で `//line` 調整と dedup が変わる（.go 以外を指す `//line`、uniq-by-line: false） | gochecksumtype.rs | unsure | S | — |

### iface-canonicalheader

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | canonicalheader literal_string.go / constant_string.go | message | `use %q instead of %q` に統一 | guff-style/src/canonicalheader.rs check_call | needs-port | S | const/, common/, alias/ ほか |
| 2 | canonicalheader analyzer.go | behavior | initialism 表が「抑制」から「正解の綴り」に | canonicalheader.rs canonical_header_key | needs-port | S | initialism/initialism.go{,.golden} |
| 3 | canonicalheader exclusions | new-option | exclusions / use-default-exclusions | canonicalheader.rs, settings.rs | needs-port | M | exclusions/*, canonicalheader_custom.{go,yml} |
| 4 | canonicalheader SuggestedFix 文言 | message | `should be replaced %q with %q` | check_call | needs-port | S | — |
| 5 | canonicalheader nil ガード | behavior | `(h.Get)("x")` で panic していた | canonicalheader.rs is_header_method | already-matches | S | — |
| 6 | iface unusedmethod（新規） | new-check | interface の未使用メソッド | guff-style/src/iface.rs | needs-port | M | unusedmethod/testdata/**, iface_unusedmethod.{go,yml} |
| 7 | iface unused 照合 | behavior | 名前一致からオブジェクト一致へ | iface.rs check_unused | already-matches | S | unused/testdata/src/basic/* |
| 8 | iface unused TypeSpec Doc | behavior | spec 単位の `//iface:ignore`（identical でも未対応） | iface.rs | needs-port | M | unused/ignoredirective/*, identical/ignoredirective/* |
| 9 | iface unused exclude | behavior | TrimSpace と空除去（カンマ分割は 2.12 時点から乖離） | iface.rs, settings.rs | needs-port | S | unused/excludepkg/* |
| 10 | iface unused SuggestedFix | behavior | 削除範囲に Doc を含む | iface.rs（fix 無し） | needs-port | S | — |
| 11 | iface opaque | behavior | named return への代入を集める | 無し | needs-port | L | opaque/testdata/** |
| 12 | iface opaque 位置 | position | 型式の位置へ | 無し | needs-port | S | opaque/n/* |
| 13 | iface unexported checkType | behavior | 内側の Ident まで剥がす | 無し | needs-port | M | unexported/testdata/** |
| 14 | iface unexported formatType | message | types.TypeString 表記 | 無し | needs-port | S | unexported/method, typeparams |

### formatters（gofumpt v0.9.2/v0.10 → v0.12、gofmt）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | format.go Options.Extra | new-option | extra の 3 規則（extra-rules は 3 つ全部） | guff-lint/src/config.rs, guff-fmt/src/gofumpt.rs, native/gofumpt/fumpter.rs | needs-port | M | func-merge-parameters / clothe-returns / diagnose.txtar |
| 2 | format.go CallExpr | behavior | 括弧揃えが BalanceCalls 下、一方向だけ | fumpter.rs call_post | needs-port | S | call-multiline.txtar |
| 3 | format.go ParenExpr | behavior | 括弧除去が既定有効、keepParens 再帰 | fumpter.rs can_remove_parens | needs-port | M | paren-remove.txtar |
| 4 | format.go File（100 バイト） | behavior | 1 行 func が 100 バイト超なら multi | fumpter.rs file_rules | needs-port | S | decls-separated.txtar |
| 5 | format.go File（effectiveEnd） | behavior | 行末コメントは前の宣言の末尾、起点は Name.End() | fumpter.rs file_rules | needs-port | S | decls-separated.txtar |
| 6 | format.go removeParens + join | behavior | 単一 spec の括弧外しを結合より前に、Rparen を end-1 | fumpter.rs join_lone_decls | needs-port | M | decl-group-single.txtar, decls-separated.txtar |
| 7 | format.go AssignStmt | behavior | 間にコメントがあれば詰めない | fumpter.rs walk_stmt | needs-port | S | assignment-newlines.txtar |
| 8 | format.go joinStdImports | behavior | コメント付き import を移さない、元の行を消す | fumpter.rs join_std_imports | needs-port | M | std-imports.txtar |
| 9 | format.go commentGroupLooksLikeCode | behavior | コメントアウトされたコード | fumpter.rs | already-matches | S | comment-code.txtar |
| 10 | format.go rxShebangComment | behavior | shebang | fumpter.rs rx_shebang | already-matches | S | comment-shebang.txtar |
| 11 | format.go diagnose | behavior | `-extra=...` 列挙とバージョン文字列（旧書式のファイルが未整形になる） | fumpter.rs fix_comments / Extra::string | needs-port | S | diagnose.txtar, gomod.txtar |
| 12 | format.go shouldMergeAdjacentFields | behavior | 印字文字列で比較 | fumpter.rs should_merge | already-matches | S | func-merge-parameters.txtar |
| 13 | govendor printer indentList | substrate | 複合リテラルを multi-line に数えない | guff-ast/src/printer/nodes.rs | needs-port | S | — |
| 14 | govendor printer intersperseComments | substrate | 次トークンが IDENT なら doc 再整形しない | guff-ast/src/printer/printer.rs | needs-port | S | — |
| 15 | govendor printer exprList log2ish | substrate | 近似関数（ビット一致で移植） | nodes.rs:331 | needs-port | S | — |
| 16 | govendor doc/comment std.go | substrate | std 一覧に `uuid` | guff-ast/src/doc/comment/std_pkgs.rs | needs-port | S | — |
| 17 | format.go File（multi の pos） | behavior | multi をコメント補正後の pos で（#5 と同時に） | fumpter.rs file_rules | needs-port | S | decls-separated.txtar |

### small-a

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | nonamedreturns analyzer.go Reportf | position | 報告位置を名前付き戻り値の識別子に | guff-style/src/nonamedreturns.rs check_results | needs-port | S | default-config/*, report-error-in-defer/* |
| 2 | nonamedreturns defer 免除 | behavior | defer 内参照 + 代入 / 値付き return | nonamedreturns.rs | already-matches | S | 同上 |
| 3 | nonamedreturns allow-unused-named-returns | new-option | 新オプション | settings.rs, nonamedreturns.rs | already-matches | S | allow-unused-named-returns/* |
| 4 | dupword skip-raw-strings | new-option | raw 文字列を検査しない | guff-comment/src/dupword.rs | **done (PR 2)** | S | raw_string_sql/a.go, dupword_skip_raw_strings.{go,yml} |
| 5 | dupword raw fix | behavior | raw のまま書き戻す | dupword.rs check_string_lit | **done (PR 2)** | S | raw_string_multiline/*, raw_string_dup/a.go |
| 6 | dupword checkOneKey 末尾 | behavior | 末尾空白直前の単語を比較（最終バイトを rune 扱い） | dupword.rs check_one_key | **done (PR 2)** | M | raw_string_multiline/* |
| 7 | tagalign find | behavior | 離れたインライン struct フィールドもグループに | guff-style/src/tagalign.rs:166 | needs-port | S | — |
| 8 | noinlineerr errMessage | message | `=` 代入用の文言 | guff-style/src/noinlineerr.rs:50 | needs-port | S | a/main.go{,.golden} |
| 9 | noinlineerr shadow チェック | behavior | `:=` のときだけ（親スコープのみの Lookup） | noinlineerr.rs:193 | needs-port | S | a/main.go{,.golden} |
| 10 | wsl checkCuddlingMaxAllowed | behavior | LabeledStmt を剥がして判定 | guff-style/src/wsl_v5.rs check_cuddle_blockish | needs-port | S | default_config/if/if.go{,.golden} |
| 11 | protogetter typesNamed | behavior | エイリアス経由のメッセージ | guff-style/src/protogetter.rs expr_named_type | needs-port | S | — |
| 12 | godoclint stdlib.json | behavior | 179 package / 11,183 symbol | guff-comment/src/godoclint_stdlib.rs | needs-port | S | require_stdlib_doclink/normal/missing.go |
| 13 | clickhouselint chbatchclose | behavior | 即時実行 defer クロージャ内の Close | guff-style/src/clickhouselint.rs | already-matches | S | chbatchclose/testdata/** |

### small-b

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | errcheck selectorAndFunc / baseCallExpr | behavior | 型引数付き呼び出しの照合 | guff-errcheck/src/lib.rs:658 | already-matches | S | TestTypeParameterizedFunctionExclude |
| 2 | errcheck DefaultExcludedSymbols | behavior | sha3 の 3 シンボル | guff-errcheck/src/excludes.rs:38 | already-matches | S | testdata/sha3.go |
| 3 | gomoddirectives replace-allow-all | new-option | replace をすべて許可 | guff-import/src/gomoddirectives.rs | **done (PR 2)** | S | testdata/replace/go.mod |
| 4 | gomoddirectives ignore-forbidden | new-option | ignore ディレクティブを禁止 | gomoddirectives.rs, gomod.rs | **done (PR 2)** | M | testdata/ignore/go.mod |
| 5 | gomoddirectives checkIgnoreDirectives | new-check | 既定で ignore される dir の指定を報告（`..` も） | 無し | **done (PR 2)** | M | testdata/ignore_defaults/go.mod（repo から） |
| 6 | tagliatelle report | behavior | json `embed` フラグを skip | guff-style/src/tagliatelle.rs:265 | needs-port | S | — |
| 7 | recvcheck 既定除外 | default-change | Marshal 系 → Unmarshal 系 | guff-style/src/recvcheck.rs:25 | needs-port | S | builtinmethods/valuetype.go |
| 8 | ginkgolinter `.Error()` チェーン | behavior | ErrorMethodPayload | guff-style/src/ginkgolinter.rs（DEFERRED） | needs-port | L | — |
| 9 | bodyclose `//bodyclose:handled` | new-check | callee の Doc ディレクティブ | guff-context/src/bodyclose.rs | needs-port | M | handledresponse/*, consumption/consumption.go |
| 10 | loggercheck checkStringerValues | new-check | nil で panic しうる Stringer 値 | guff-style/src/loggercheck.rs | needs-port | M | — |
| 11 | loggercheck 引数数ガード | behavior | 多値展開で panic | loggercheck.rs:375 | already-matches | S | issue108 |
| 12 | fatcontext 2 フラグ | new-option | check-loops / check-function-literals | settings.rs:942, fatcontext.rs:291 | already-matches | S | fatcontext*.go/yml |
| 13 | fatcontext isRunOnce | behavior | defer IIFE と t.Cleanup を報告しない | guff-context/src/fatcontext.rs run | needs-port | M | common/example.go, no_*/example.go |
| 14 | unparam addSrcFunc（AnonFuncs） | behavior | 到達不能な func literal も検査 | guff-style/src/unparam.rs:2048, :1117 | needs-port | M | unexported_methods.txtar |
| 15 | unparam 非公開型のメソッド | behavior | issue #91 | unparam.rs | already-matches | S | unexported_methods.txtar |
| 16 | unparam linknameDoc | behavior | `//go:linkname` を skip | unparam.rs check_func_decl | needs-port | S | linkname.txtar |
| 17 | unparam signRequiredBy | behavior | go / defer 引数、MapUpdate、Send、Select | unparam.rs collect_sign_required | needs-port | S | usedas.txtar |
| 18 | unparam returnValues / storedValue | behavior | store → load を辿る | unparam.rs collect_results_required | needs-port | M | samerets.txtar |
| 19 | unparam eqlConsts | behavior | types.Identical で比較 | unparam.rs consts_equal | needs-port | S | typealias.txtar |
| 20 | unparam error alias | behavior | `type E = error` を除外 | unparam.rs is_error_type | needs-port | S | typealias.txtar |
| 21 | unparam containsTypeParam | behavior | ゼロサイズ skip（guff にサイズ gate 自体が無い） | unparam.rs check_params | needs-port | S | typealias.txtar, typeparams.txtar |
| 22 | unparam findNamed Origin | behavior | 実体化経由の interface 実装 | unparam.rs collect_types_implementing | already-matches | S | typeparams.txtar |

---

## 5. drift で見えていて、コード差分で説明できないもの

### 5.1 golden ゲート（guff 対 2.14.0）の各行の内訳

drift report の「What the golden gate would say」を、台帳の項目と既存 ratchet に割り付けた。
PR 1 で実際に pin を上げて測った値（2026-10-04、release バイナリ）はこの表と一致し、各 case の `ratchet.json` に記録した。
**割り付けられなかったものだけが本当の「未説明」**。

| case | ゲート | 説明 |
|------|--------|------|
| canonicalheader | missing 10 / extra 6 | 文言（iface-canonicalheader #1）6/6 + initialism（#2）4 |
| exhaustive / -default-case-required | 8 / 0 ずつ | ディレクティブ厳密一致（exhaustive-sumtype #1）、不正ディレクティブ診断（#2）、alias（#5） |
| exhaustive-explicit | 4 / 2 | 同上 |
| gocritic | 1 / 0 | sprintfQuotedString（gocritic-goconst #1） |
| gofumpt | 1 / 0 | 100 バイト規則（formatters #4） |
| gosec | 6 / 8 | G118 ×5（#2）、G115（#3）、G703（#4）、G404 ×2（#5）は説明済み。**G702 g7xx:73:18 の消滅と G124 g124_elided の +3 は gosec のコード差分では説明できない**（x/tools の compLit lowering と推定、§2.1） |
| govet-fieldalignment | 14 / 14 | 文言（xtools-passes #1） |
| modernize | 19 / 27 | fmtappendf 14、reflecttypefor 9、slicescontains 2、stringsbuilder 2（以上 extra 27）／ errorsastype 10、reflecttypeassert 6、importcomment 1、stringscut Split 2（missing 19） |
| nonamedreturns | 14 / 14 | 報告位置（small-a #1） |
| protogetter | 1 / 0 | alias（small-a #11） |
| recvcheck | 1 / 1 | 既定除外の入れ替え（small-b #7） |
| revive | 1 / 10 | empty-block 6（revive #1）+ 既存 ratchet 1/4 |
| staticcheck-s | 1 / 2 | S1005 ×2（staticcheck-checks #3）、SA4006 s1017（staticcheck-ir #1） |
| staticcheck-sa | 18 / 22 | SA1019 文言、SA4003、SA5011 削除 ×6、SA6005、SA6001（既存 ratchet）、**sa6000/ok/ok.go:13:5 SA4006 の消滅**（下記）、`stdlib.go:41:51 (crypto/ecdsa.PublicKey).X`（下記） |
| staticcheck-sa1019-* 6 件 | 計 42 / 42 | SA1019 の SelectorName（staticcheck-checks #1、#2）。`-protoc-gen-go`（2/2）は drift の測定後に #471 で足された case |
| staticcheck-st | 8 / 0 | 既存 ratchet 10 から st1005 SA4017 ×2 が上流で消えた分（guff は元から出していない） |
| staticcheck-qf | 1 / 0 | 既存 ratchet（SA4017 の cross-package purity）。bump とは無関係 |
| unparam | 1 / 0 | AnonFuncs（small-b #14） |
| unused | 0 / 2 | Unify（staticcheck-checks #15） |
| wastedassign | 4 / 2 | go/ssa assign（§2.1） |
| inline-gofix-sibling | 2 / 0 | 既存 ratchet（インライナ未移植）。bump とは無関係 |
| golines / swaggo | 一致 | **2026-10-04 に手元で測って一致**（drift の CI 実行環境に外部バイナリが無かっただけ）。ratchet 不要 |

### 5.2 未説明として残るもの

1. **staticcheck-sa `sa6000/ok/ok.go:13:5` SA4006 `this value of match is never used`（-2.12.2）** ——
   sa4006.go の差分（IncDec / 複合代入の追加、Sigma 枝削除）は報告を**減らす**方向ではない。
   go/ir の lift / ValueForExpr / blockopt 側と推測するが、特定できる hunk が無い。guff は今これを報告しているので 2.14.0 に対して extra になる。
2. **staticcheck-st `st1005/local_errors/local_errors.go:7:2, 8:2` SA4017（-2.12.2）** ——
   sa4017.go の差分は FilterDebug の除去だけ。依存先の `New` は `return nil` だけで v0.7 の IsStub でも stub のはず。
   guff は既に出していない（ratchet missing）ので一致方向。原因は未特定。
3. **gosec G702 `g7xx/g7xx.go:73:18`（-2.12.2）と G124 `g124e/g124_elided.go:77:65, 79:67, 81:56`（+2.14.0）** ——
   gosec 側は未変更。x/tools go/ssa の compLit lowering 変更と推定。§2.1 の wastedassign と同じ PR で確かめる。
4. **staticcheck-sa `stdlib.go:41:51 (crypto/ecdsa.PublicKey).X`（+2.14.0）** ——
   honnef の `knowledge/deprecated.go` 差分は `(crypto/tls.Config).Rand` の追加だけで、ecdsa は差分に無い。
   guff は既に `stdlib_deprecations.rs:197` に持っていて報告している（一致方向）。
   上流で 2.12.2 が黙っていた理由（解析対象 module の Go 版判定か、ビルド toolchain か）は未確認。
5. **Inventory `linter:modernize autoFix False -> True`** —— x/tools 側では説明できないが、golangci-core #4（WithAutoFix）で説明済み。

### 5.3 どのグループの diff にも入っていなかったファイル

- `4d63.com/gocheckcompilerdirectives` v1.3.0 → v1.4.0（mods.txt / diffs に無かった）→ golangci-core #21 で追加、already-matches。
- `github.com/sourcegraph/go-diff` v0.8.0 → v0.9.0（revgrep の extended header 解析）→ golangci-core #22、unsure。
- `golang.org/x/mod` v0.35 → v0.41 → modfile 読み取りに関係なし、no-op。
- iface `unusedmethod/doc.go`、staticcheck `internal/xtools-internal/versions/gover.go`、gofmt `internal/.gitattributes` —— いずれも観測不能、所属項目に含めた。

---

## 6. 推奨 PR 順序

原則: **最初の PR で pin を上げ、ゲートの赤を全部 ratchet に記録する**。以降の各 PR は ratchet を縮めることで進捗を示す
（「新しい golden 差分 N 件」ではなく「ratchet が a/b → c/d」で書ける）。
各 PR のローカルゲートは golden / fix / reject / workspace / **isolate**（golden / fix / reject / workspace が全部緑でも isolate だけ落ちたことがある）。
golden を regen したら fix のベースラインも撮り直す。測定は release バイナリで。

| # | PR | 中身 | 縮む ratchet | 規模 |
|---|----|------|--------------|------|
| 1 | **pin bump + golden regen** | `compat/pins.json` と `GOLANGCI_LINT_COMPAT` を 2.14.0 に、全 golden / fix / reject を regen（2 回一致）。§5.1 の各行を case ごとの `ratchet.json` に、fix の不足を `pending/` に記録。drift-ledger は不要（pin == 最新で 0 件）。**加えて over-fix 5 項目を移植した**: fix tier は guff が上流より多く書き換える case を pending に置けず、`divergent/` は「guff が正しい」場合専用なので、S1005 の map comma-ok（staticcheck-checks #3）と modernize の Suite 入れ替え（#1, #2）・reflecttypefor `usesNonTypeSymbol`（#8）・slicescontains `NoEffects`（#9）・stringsbuilder `_test.go`（#10）をこの PR で入れた | modernize 19/27 → **2/0**、staticcheck-s → **1/0**（ここで作って即縮めた） | M |
| 2 | **config 層（実際の範囲）** | exhaustruct_v5 の名前受理（v4 エンジンで代用、v5 設定は 3 キーのみ配線・残りは警告）、deprecation 警告（exhaustruct / wsl / gomodguard）、dupword `skip-raw-strings` と v0.1.8 の挙動 2 つ、gomoddirectives v0.10（allow-all / ignore-forbidden / ignore 既定ディレクトリ / ブロック内の列）、modernize の旧名警告。**linter 本体と一体のキー（canonicalheader exclusions、goconst、gofumpt extra.*、iface、gosec global、revive）は各 linter の PR に移した** | exhaustruct-v5 3/0（新設）。dupword fix pending 消滅 | M |
| 3 | **x/tools substrate（安い方）** | wastedassign の `composite_lit_rhs` 撤去、`nillable` に unsafe.Pointer、gosec G702 / G124 の再測定（§5.2-3） | wastedassign 4/2 → 0/0、gosec 一部 | S |
| 4 | **honnef IR 寄せ** | SSI 模倣の撤去（sa4006 / sa4008 / sa5011）、DebugRef の監査、IsStub、`EnclosingFunction` の `_`、SA5011 の登録解除 | staticcheck-s SA4006、staticcheck-sa の SA5011 ×6、sa6000 SA4006 を測る | L（golden 全体を変換して一度に測る。共有ヘルパだけ直すと偶然一致していた check が壊れる） |
| 5 | **staticcheck の文言・小移植** | SA1019 SelectorName + literal キー位置、SA4003、SA6005、SA1026、SA5008 embed、SA9010（新規）、SA4006 IncDec | staticcheck-sa1019-* 42/42 → 0/0、staticcheck-sa の大半 | M |
| 6 | **modernize 残り（ゴールデン側）** | stringscut の Split / SplitN 腕（Suite 入れ替え・reflecttypefor・slicescontains・stringsbuilder は PR 1 で済み） | modernize 2/0 → 0/0 | S |
| 7 | **govet 文言** | fieldalignment（size class 表の pin を決める）、composite、inline の埋め込み判定、unusedresult b.Loop、nilness cgo | govet-fieldalignment 14/14 → 0/0 | M |
| 8 | **出力差の小物まとめ** | nonamedreturns 位置、canonicalheader fork（文言 + initialism + exclusions）、exhaustive ディレクティブ厳密一致 / 不正ディレクティブ / alias、recvcheck、protogetter alias、gocritic sprintfQuotedString、gofumpt 100 バイト規則 | nonamedreturns 14/14、canonicalheader 10/6、exhaustive ×3、recvcheck、protogetter、gocritic、gofumpt → 0 | M（項目は多いが各 S） |
| 9 | **gosec 既存ルール** | G118、G115、G703、G404、`%#q` 系と並べて TryResolve Builder（2 系統） | gosec 6/8 → 0/0 | M |
| 10 | **revive** | empty-block range、multiline-if-init を enable-all に、marshal-receiver、use-slices-concat、文言 4 本、Go 版ゲート 2 本、directives | revive 1/10 → 既存 1/4 | M |
| 11 | **unused / unparam** | Unify、unparam AnonFuncs + linkname + signRequiredBy + alias | unused 0/2、unparam 1/0 → 0 | M |
| 12 | **modernize 残り（ゴールデンに出ない側）** | minmax ×2、atomictypes、waitgroupgo、testingcontext、rangeint、slicesbackward ×3、stringscut ×3、stringscutprefix、stringsbuilder lastEditEnd、slicesclip。**各項目で上流 testdata を fixture 化**（ratchet は動かないので fixture が唯一の検算） | ― | M〜L |
| 13 | **gofumpt v0.12 の残り + printer** | §2.4 の toolchain 確認の後。括弧除去、effectiveEnd + multi、removeParens、joinStdImports、diagnose。`omit_v010_rules` / `match_golangci` gate の撤去 | ― | L |
| 14 | **新機能（大）を 1 本ずつ** | G407、exhaustruct_v5（3〜4 PR に分割：設定 → ディレクティブスキャナ → 欠落計算 → tag 移行）、gochecksumtype fact、iface unusedmethod / opaque / unexported、stdversion、goconst eval-const-expressions、embedlit | ― | 各 L |
| 15 | **Go 1.27 toolchain 対応（保留）** | export data V5、printf %d go1.27、stringscut CutLast、昇格フィールドキー。ターゲット Go が 1.27 になる日まで保留 | ― | L |

PR 1〜11 で §5.1 の ratchet は**既存の恒久差分（revive 1/4、inline-gofix-sibling 2/0、staticcheck-qf / st の SA4017）まで戻る**見込み。
PR 12 以降は ratchet が動かない（golden に形が無い）ので、fixture を足すことが進捗の定義になる。

---

## 7. 未解決の問い

1. ~~**2.14.0 リリースバイナリのビルド Go**~~ → **go1.27.0**（§2.4）。以下は当初の問い:gci の std 一覧は go1.27 で生成されている。go1.27 なら gofmt / goimports / swaggo も printer 変更を受け、型エラーの文言も変わりうる。§2.4 の切り替え単位がこれで決まる。
2. **sa6000/ok/ok.go:13:5 SA4006** と **st1005 SA4017 ×2** の消滅原因（§5.2）。前者は guff に対して extra になるので、PR 4 で IR 側を寄せた後にまだ残るか測る。
3. **typeutil.Callee の Origin 化を guff の共有ヘルパに入れる範囲**。`call_target_object` は govet 以外でも共有されている。上流で typeutil.Callee を呼ぶ linter を数え、golden 全体で実験してから寄せる（偶然一致を壊さない）。
4. **SA5011 を `staticcheck.checks` に明示した config** を 2.14.0 がどう扱うか（無視 / エラー）。reject tier に入れるか。
5. **SA1019 の composite literal 分岐で generic 実体化（`pkg.G[int]{F: ...}`）の型に当たると上流 SelectorName は panic する**。2.14.0 の出力（analyzer エラー？）を実測し、guff の再現方針を決める。
6. **`GOLANGCI_LINT_CACHE=off`** を上流は通常のディレクトリ名として扱う。guff の `GUFF_CACHE=off` 拡張と上流互換のどちらを優先するか。
7. **exhaustruct_v5 / exhaustive / gochecksumtype は依存パッケージのソース（doc コメント）を読む**。guff は依存を export data / scope から組み立てている。依存ソースを reparse する経路を 1 つ作り、3 linter で共有するか。
8. **gochecksumtype の analyzer error が goanalysis_metalinter 全体を落とす**挙動を、guff の RunError で再現するか（再現すると 1 つの壊れた `//sumtype:decl` で全 linter の issue が消える）。
9. **fieldalignment の size class 表**は golangci-lint を建てた Go runtime の表に依存する。どの Go の sizeclasses を転記するか（1. と同じ問い）。
10. **revive の未知 rule 名**: 上流 2.12.2 はエラー。guff は無視。新 rule を実装するまでのつなぎに警告かエラーを出すか。
11. **revive unhandled-error の「ホスト依存」**: `callee_is_local` は importer 盲目のホストに較正されている。COMPAT-HARDENING §6 の再較正（同一パッケージ + stdlib）をやるなら、`fmt.Fprintf(&buf)` の例外が同時に要る。
12. **sourcegraph/go-diff v0.9.0** が rename / mode 変更 / binary を含む patch での new-from-patch の行対応を変えるか。guff の diff.rs で実測が要る。
13. **G407 は 2.14.0 の jsonschema の gosec-rules enum に無い**（G406 の次が G408）が、実行時は既定で有効。guff の設定検証はスキーマではなくコードに合わせる（includes / excludes に G407 を受理する）。
14. **goconst `exclude-types` を明示したとき viper が既定 [Call] を置換するか**を実機で確認（台帳はコードから「置換」と読んだ）。
15. **gofumpt `extra-rules` の deprecation 警告文言と挙動の食い違い**（警告は group-params だけを案内、コードは 3 規則全部）。警告は文言どおり転記、挙動はコードに合わせる、で良いか。
16. **upstream testdata の取得**: revive v1.17.0、exhaustruct v5.2.0 の analyzer/testdata、gomoddirectives v0.10.0 の ignore_defaults、loggercheck の issue108 は module zip に無い。各 repo のタグから取る必要がある。
17. **unparam のゼロサイズ型 skip**（`stdSizes.Sizeof == 0`）は guff に元から無い。bump とは独立の既存乖離だが、2.14.0 の containsTypeParam 変更で乖離の形が変わるので、PR 11 で gate ごと入れるか。
18. **golines / swaggo の missing 1**: drift 実行環境に外部バイナリがあったかを確認し、無かったなら drift.py が「バイナリ不在」を区別して報告するようにする。

---

## 付録 A. 読んだ module と版（golangci-lint v2.12.2 / v2.14.0 の go.mod）

diff は `go mod download -json <mod>@<ver>` で得た 2 つの Dir を `diff -ruN -x '*.md' -x go.sum -x .github` したもの。
ローカルの上流 checkout は pin 版ではないので使わない。

| 名前 | 旧 module@版 | 新 module@版 |
|---|---|---|
| golangci-lint | `github.com/golangci/golangci-lint/v2@v2.12.2` | `github.com/golangci/golangci-lint/v2@v2.14.0` |
| exhaustruct | `dev.gaijin.team/go/exhaustruct/v4@v4.0.0` | `dev.gaijin.team/go/exhaustruct/v5@v5.2.0` |
| tagalign | `github.com/4meepo/tagalign@v1.4.3` | `github.com/4meepo/tagalign@v1.4.4` |
| dupword | `github.com/Abirdcfly/dupword@v0.1.7` | `github.com/Abirdcfly/dupword@v0.1.8` |
| gochecksumtype | `github.com/alecthomas/go-check-sumtype@v0.3.1` | `github.com/alecthomas/go-check-sumtype@v0.5.1-0.20260828200218-ae6904d28606` |
| noinlineerr | `github.com/AlwxSin/noinlineerr@v1.0.5` | `github.com/AlwxSin/noinlineerr@v1.0.6` |
| errname | `github.com/Antonboom/errname@v1.1.1` | `github.com/Antonboom/errname@v1.1.2` |
| nilnil | `github.com/Antonboom/nilnil@v1.1.1` | `github.com/Antonboom/nilnil@v1.1.2` |
| wsl | `github.com/bombsimon/wsl/v5@v5.8.0` | `github.com/bombsimon/wsl/v5@v5.9.0` |
| mirror | `github.com/butuzov/mirror@v1.3.0` | `github.com/butuzov/mirror@v1.3.3` |
| clickhouselint | `github.com/ClickHouse/clickhouse-go-linter@v1.2.0` | `github.com/ClickHouse/clickhouse-go-linter@v1.2.1` |
| nonamedreturns | `github.com/firefart/nonamedreturns@v1.0.6` | `github.com/firefart/nonamedreturns@v1.0.8` |
| protogetter | `github.com/ghostiam/protogetter@v0.3.20` | `github.com/ghostiam/protogetter@v1.0.1` |
| gocritic | `github.com/go-critic/go-critic@v0.14.3` | `github.com/go-critic/go-critic@v0.15.0` |
| godoclint | `github.com/godoc-lint/godoc-lint@v0.11.2` | `github.com/godoc-lint/godoc-lint@v0.11.4` |
| canonicalheader | `github.com/lasiar/canonicalheader@v1.1.2` | `github.com/golangci/canonicalheader@v0.0.0-20260827115959-a25c71c521f6` |
| gofmt | `github.com/golangci/gofmt@v0.0.0-20250106114630-d62b90e6713d` | `github.com/golangci/gofmt@v0.0.0-20260820135601-e84e05053792` |
| goconst | `github.com/jgautheron/goconst@v1.10.0` | `github.com/jgautheron/goconst@v1.11.0` |
| errcheck | `github.com/kisielk/errcheck@v1.10.0` | `github.com/kisielk/errcheck@v1.20.0` |
| gomoddirectives | `github.com/ldez/gomoddirectives@v0.8.0` | `github.com/ldez/gomoddirectives@v0.10.0` |
| tagliatelle | `github.com/ldez/tagliatelle@v0.7.2` | `github.com/ldez/tagliatelle@v0.8.0` |
| revive | `github.com/mgechev/revive@v1.15.0` | `github.com/mgechev/revive@v1.17.0` |
| exhaustive | `github.com/nishanths/exhaustive@v0.12.0` | `github.com/nishanths/exhaustive@v0.13.0` |
| ginkgolinter | `github.com/nunnatsa/ginkgolinter@v0.23.0` | `github.com/nunnatsa/ginkgolinter@v0.24.0` |
| recvcheck | `github.com/raeperd/recvcheck@v0.2.0` | `github.com/raeperd/recvcheck@v0.3.1` |
| gosec | `github.com/securego/gosec/v2@v2.26.1` | `github.com/securego/gosec/v2@v2.29.0` |
| bodyclose | `github.com/timakin/bodyclose@v0.0.0-20260129054331-73d1f95b84b4` | `github.com/timakin/bodyclose@v0.0.0-20260723120731-857993a2939c` |
| loggercheck | `github.com/timonwong/loggercheck@v0.11.0` | `github.com/timonwong/loggercheck@v0.12.0` |
| iface | `github.com/uudashr/iface@v1.4.2` | `github.com/uudashr/iface@v1.5.1` |
| fatcontext | `go.augendre.info/fatcontext@v0.9.0` | `go.augendre.info/fatcontext@v0.10.1` |
| xtools | `golang.org/x/tools@v0.44.0` | `golang.org/x/tools@v0.50.0` |
| staticcheck | `honnef.co/go/tools@v0.7.0` | `honnef.co/go/tools@v0.8.1` |
| gofumpt | `mvdan.cc/gofumpt@v0.9.2` | `mvdan.cc/gofumpt@v0.12.0` |
| unparam | `mvdan.cc/unparam@v0.0.0-20251027182757-5beb8c8f8f15` | `mvdan.cc/unparam@v0.0.0-20260823230713-2fa3d841b0c8` |
