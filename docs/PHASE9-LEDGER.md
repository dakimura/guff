# Phase 9 — golangci-lint 2.14.0 → 2.15.0 ＋ Go 1.26 → Go 1.27 移植台帳

Phase 8（`docs/PHASE8-LEDGER.md`）と同じ作り方: 出力ではなく**上流のコード差分**から行を起こし、
`compat/drift.py --candidate 2.15.0` の結果で検算した。

2 つを 1 つの Phase にまとめた理由: 上流の golangci-lint 2.14.0 のバイナリ自体がすでに go1.27.0 でビルドされており
（`go version compat/.tools/golangci-lint-2.14.0/golangci-lint`）、2.15.0 は x/tools を 0.50→0.52 に上げている。
利用者側の toolchain も 2026-11 時点で go1.27.3 が最新である。

> 作成: 2026-11-02。読み取り専用で作成した（guff は一切編集していない）。各行は guff のコードを読んで判定し、
> 多くは 2.15.0 / 2.14.0 / guff のバイナリを scratch の fixture で走らせて確かめた。
> 「guff の場所」は `crates/` を省略して書く。凡例は Phase 8 と同じ（status = needs-port / already-matches / not-applicable / unsure、effort = S / M / L）。

---

## 1. 概要

### 1.1 実測した事実（台帳を作る前）

| 測ったこと | 結果 |
|---|---|
| OSS pr tier を Go 1.27 で（2.14.0 vs guff） | **注意: 最初の測定（`GOTOOLCHAIN=go1.27.3` だけ）は無効**。guff は stdlib のソースを PATH 上の `go` の GOROOT から読む（`compat.yml` の GOTOOLCHAIN コメント）ので、`go list` は 1.27・guff の stdlib は 1.26.5 の混成になっていた。PATH に go1.27.3 を置き `GOTOOLCHAIN=local` で測り直したのは generic method 対応後のバイナリで、**8/8 target 一致** |
| Go 1.27 の generic method（`func (T) M[U any]()`）を含むファイル | guff の parser が `method must have no type parameters` で**そのファイルを黙って捨てる**。他の linter の finding も全部消え、exit 0。golangci は普通に解析する |
| export data | go1.27 は pkgbits **V5** を書く。guff-exportdata は V0–V2 だけ。`GUFF_DEP_SOURCE=0`（export data 経路）では go1.27 で**黙って 0 件** |
| `math/rand/v2`（stdlib 自身が generic method を持つ）を import するだけ | 上と同じ混成環境での測定だったので**無効**（guff は 1.26.5 の `math/rand/v2` を読んでいた）。本物の go1.27 環境では、対応前の parser は `rand.go:213` の `func (r *Rand) N[Int intType]` を捨てる。対応後のバイナリでは正しく出る |
| `compat/drift.py --candidate 2.15.0`（golden 366 case） | 上流の出力が動いたのは 10 case: canonicalheader ×2、exhaustive ×4、iface-edges、loggercheck、predeclared、revive。inventory は predeclared が `fast` から外れた 1 件 |

**測るときの注意**: Go 1.27 で測るときは `PATH=<go1.27 の bin>:$PATH GOTOOLCHAIN=local` にする。`GOTOOLCHAIN=go1.27.3` だけでは guff が古い stdlib を読む。

### 1.2 グループ別の行数

| グループ | needs-port | already-matches | not-applicable | unsure | 計 |
|---|---:|---:|---:|---:|---:|
| go-syntax（parser・scanner・printer・pkgbits・gcimporter） | 7 | 9 | 9 | 1 | **26** |
| go-types（go/types 1.27） | 17 | 3 | 8 | 0 | **28** |
| xtools（x/tools 0.50→0.52） | 14 | 7 | 14 | 0 | **35** |
| golangci-core ＋ staticcheck 0.8.2 | 14 | 5 | 13 | 0 | **32** |
| linters-a（exhaustive・predeclared・gocognit・iface） | 36 | 8 | 6 | 0 | **50** |
| linters-b（errcheck・revive・loggercheck・wrapcheck・paralleltest・rowserrcheck・canonicalheader・golines・bidichk） | 23 | 3 | 13 | 2 | **41** |
| **合計** | **111** | 35 | 63 | 3 | **212** |

（Phase 8 は needs-port 198 行で、2026-10-05 から約 4 週間かかった。）

### 1.3 Phase 9 中に見つかった既存の差（1.27 / 2.15 の差分ではない）

- **型パラメータの switch**（`func g[P T](p P){ switch p {} }`、制約が interface でない型）を含むパッケージを guff が**丸ごと黙って落とす**。golangci は普通に解析する（linters-a の付記）。**done (P9-4)**：`comparable_type` が型パラメータの underlying を `iface()`（暗黙の `interface{ T }`）で取るように。残る差は exhaustive が型パラメータの switch を見ないこと（exhaustive v0.15 の作り直しで扱う）。
- guff は import cycle を finding にしない（xtools 台帳）。
- `linters.default: fast` が 4 linter しか有効にしない（上流は 44 個）（core #11）。
- `guff custom` の版文字列・cache salt にプラグインの版が入らない（core #1）。
- export data 経路（`GUFF_DEP_SOURCE=0`）はもとから壊れている（go-syntax #26）。あわせて、その経路で `math/rand/v2` のように path の末尾と package 名が違う package を `v2` と名付けていた（P9-2 で修正）。
- guff は parse error / 型エラーを `typecheck` finding にしない（COMPAT-HARDENING 続き 150–151 の既知の判断）。go/types 台帳の文言行（#13/#15/#16/#19/#25）はこれが入るまで出力に効かない。

---

## 2. PR 順序（全体）

優先度は「guff が**黙って**結果を落とすもの」を最上位に置く。

| 順 | PR | 中身（台帳の行） | effort |
|---|---|---|---|
| 1 | **generic method を通す** | go-syntax #1（parser）＋ go-types T1（#5 #6 #7）。SSA 側（guff-ssa）で generic method が落ちないことも fixture で確認 | M〜L |
| 2 | **export data V3–V5** | go-syntax PR A ＋ B（#16–#22、#26 の失敗の見え方） | M〜L |
| 3 | **1.27 の型検査の残り** | go-types T3（関数型推論 #14）、T4（struct literal の昇格フィールド #12 #13）、T2、T5 | M×2＋S×2 |
| 4 | **型パラメータ switch の既存バグ** | 1.3 の 1 行目 | S〜M |
| 5 | **pin を 2.15.0 へ** | core PR A（golden / fix / reject の regen、drift の review） | S |
| 6 | 2.15 の linter 群 | exhaustive（L、typeswitch は別 L）、predeclared（M）、gocognit（M）、iface（S）、errcheck（M）、revive（M×2＋S×7）、canonicalheader・paralleltest・loggercheck ゲート・golines pin（S）、wrapcheck（M）、loggercheck nilness（L） | 計 L×4 程度 |
| 7 | x/tools 0.52 | SSA の評価順 2 件（M）、modernize の過剰報告 3 形・stringscut fix（M）、stringintconv fix（M）、goimports embed（S）、import cycle（M〜L） | M×4 |
| 8 | staticcheck 0.8.2・core の小物 | SelectorName（S）、custom 版 hash（M）、`fast` 集合（M）、警告類（S） | M×2＋S×4 |
| 9 | 低優先 | go-types T6/T7（文言・不正コード専用）、nilness DFA（Phase 8 staticcheck-ir #7 に合流、L）、revive struct-tag チェッカー（L） | — |

見積もり（Phase 8 の速度で）: 1〜4 が 1.5〜2 週、5〜8 が 1.5〜2 週。合計 **3〜4 週**。

---

## 付録 A. 読んだ module と版

GOROOT は go1.26.5 → go1.27.3（`*_test.go` と testdata を除く。fixture は新 GOROOT の testdata を参照）。
module は `go mod download -json <mod>@<ver>` で得た 2 つの Dir を `diff -ruN -x '*.md' -x go.sum -x .github` したもの。

| 名前 | 旧 | 新 |
|---|---|---|
| golangci-lint | `github.com/golangci/golangci-lint/v2@v2.14.0` | `@v2.15.0` |
| bidichk | `github.com/breml/bidichk@v0.3.3` | `@v0.3.4` |
| canonicalheader | `github.com/golangci/canonicalheader@v0.0.0-20260827115959-a25c71c521f6` | `@v0.0.0-20260928145534-e18e5c5a078f` |
| golines | `github.com/golangci/golines@v0.15.0` | `@v0.16.0` |
| rowserrcheck | `github.com/golangci/rowserrcheck@v0.0.0-20260419091836-c5f79b8a11ba` | `@v0.0.0-20261005233414-e0740d52dee8` |
| errcheck | `github.com/kisielk/errcheck@v1.20.0` | `@v1.30.0` |
| paralleltest | `github.com/kunwardeep/paralleltest@v1.0.15` | `@v1.0.16` |
| revive | `github.com/mgechev/revive@v1.17.0` | `@v1.17.1` |
| exhaustive | `github.com/nishanths/exhaustive@v0.13.0` | `@v0.15.0`（testdata は module zip に無いので GitHub のタグから） |
| predeclared | `github.com/nishanths/predeclared@v0.2.2` | `@v0.3.0` |
| loggercheck | `github.com/timonwong/loggercheck@v0.12.0` | `@v0.13.0` |
| wrapcheck | `github.com/tomarrell/wrapcheck/v2@v2.12.0` | `@v2.13.0` |
| gocognit | `github.com/uudashr/gocognit@v1.2.1` | `@v1.3.0` |
| iface | `github.com/uudashr/iface@v1.5.1` | `@v1.5.3` |
| xtools | `golang.org/x/tools@v0.50.0` | `@v0.52.0` |
| staticcheck | `honnef.co/go/tools@v0.8.1` | `@v0.8.2` |

---

## 3. グループ別の台帳

> 本文中の `scratchpad/phase9/...` は作成時の実験ディレクトリで、リポジトリには無い。


### 3.1 go-syntax

###### goroot-syntax（go/parser・scanner・token・ast・printer・build、internal/pkgbits、go/internal/gcimporter：go1.26.5 → go1.27.3）

凡例は §4 と同じ。「guff の場所」は `crates/` を省略。読み取り専用で作成（guff は編集していない）。
`already-matches` は guff の該当行を読んで確認したものだけ。各行は作成後に反証を試み、根拠が残ったものだけを残した。

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | go/parser/parser.go parseFuncDecl | behavior | **generic method を受理する**。`recv != nil && tparams != nil` のエラー（"method must have no type parameters"）と `tparams = nil` を削除。FuncDecl.Type.TypeParams に残る | guff-ast/src/parser.rs:2836-2845（エラーを出して tparams を捨てる。go1.26 のまま） | **done (P9-1)** | S（parser 単体）。効果が出るのは guff-types 側が揃ってから | go/parser/testdata/issue50427.go2（ERROR 注記が消えた）、go/internal/gcimporter/testdata/genmeth.go、internal/types/testdata/spec/methods.go、fixedbugs/issue50427{a,b}.go。**実物: GOROOT math/rand/v2/rand.go:213 `func (r *Rand) N[Int intType]`**（go1.26 は package 関数 N だけ） |
| 2 | go/scanner + go/parser（scannerhooks.StringEnd → Scanner.End()） | substrate | STRING の終端を backdoor hook ではなく `s.file.Pos(s.offset)` で取る。BasicLit.ValueEnd・ImportSpec の path 終端・struct tag 終端が全部 `p.end()` に。INT/FLOAT/CHAR の `pos+len(lit)` も `End()` に置き換わったが、lit はどれも `src[offs:offset]` なので値は同じ。挙動は不変のリファクタ | guff-ast/src/scanner.rs:944,956（string_end）、parser.rs:280,752,1565-1569,2666（STRING は string_end、他は pos+lit.len()） | already-matches | S | go/scanner/scanner_test.go TestScannerEnd（期待値の表として流用可） |
| 3 | go/scanner/scanner.go Scanner.End / endPos | new-option | 公開 API `Scanner.End()` 新設。`/*\n*/` 内の改行から合成される SEMICOLON の終端は nlPos+1 | —（guff の scanner に End 相当なし。呼ぶ側も無い。x/tools v0.52 の analysis/internal にも Scanner.End の呼び出しは無い） | not-applicable | S | scanner_test.go TestScannerEnd |
| 4 | go/scanner/scanner.go Init | behavior | Init が `*s = Scanner{...}` で全状態を初期化（go1.26 は nlPos を戻さず、再利用した Scanner が前回の合成 SEMICOLON を返した） | guff-ast/src/scanner.rs:141-160（init で nl_pos / string_end / error_count / insert_semi を全部リセット） | already-matches | S | scanner_test.go TestScanReuseSemiInNewlineComment |
| 5 | go/scanner/errors.go `var _ error = (*Error)(nil)`、go/build/read.go `&scanner.Error{}` | other | エラーの型（値 → ポインタ）だけ。メッセージ文字列は不変 | — | not-applicable | S | — |
| 6 | go/token/position.go File.String | new-option | `name(base-end)` を返す String() 新設。デバッグ用 | — | not-applicable | S | — |
| 7 | go/token/position.go fixOffset | substrate | 非 debug 時は `max(min(size, off), 0)` に整理。値は go1.26 と同じ clamp | guff-ast/src/position.rs:281-289（<0→0、>size→size） | already-matches | S | — |
| 8 | go/ast/filter.go MergePackageFiles | behavior | `i++` をループ末尾へ。go1.26 は i が 0 にならず minPos が常に 0 のままだった（maxPos は結果同じ）。go1.27 は最初のファイルの FileStart から min を取る | guff-ast/src/filter.rs:330-342（enumerate の idx==0 で初期化＝go1.27 の挙動。呼び出し元は crate 内に無い） | already-matches | S | go/ast/filter_test.go |
| 9 | go/ast/ast.go, scope.go の doc comment | other | doc link 表記の修正だけ | — | not-applicable | S | — |
| 10 | go/printer/nodes.go exprList + math.go | behavior | key 揃えの幾何平均を `math.Log/Exp` から `log2ish/exp2ish`（アーキテクチャ非依存の近似）に変更。揃えの切れ目が変わりうる（go.dev/issue/77959） | guff-ast/src/printer/nodes.rs:331,355,375,393（log2sum）、1581-1610（log2ish / exp2ish / frexp。Frexp の [0.5,1) と指数を確認済み） | already-matches（Phase 8 §2.4 で移植済み） | S | go/printer/testdata/alignment.{input,golden}（末尾の Issue #77959 ケース） |
| 11 | go/printer/nodes.go indentList / isCompositeLitLike | behavior | return 文の式リストで、複数行の複合リテラル（`&T{}`・括弧も含む）を「複数行要素」と数えない（go.dev/issue/7195） | guff-ast/src/printer/nodes.rs:1079-1101、is_composite_lit_like（1567-1579、strip_parens・UnaryExpr AND を確認） | already-matches（Phase 8 §2.4） | S | go/printer/testdata/statements.{input,golden}（85-88 行の差） |
| 12 | go/printer/printer.go intersperseComments | behavior | 次トークンが IDENT のときは「1 桁目の abutting コメント＝doc comment」の再整形（formatDocComment）をしない | guff-ast/src/printer/printer.rs:833-837（`tok != Token::IDENT` を確認） | already-matches（Phase 8 §2.4） | S | —（printer の testdata に専用 case は無い。fixture は自作） |
| 13 | go/build/build.go Import（UseAllFiles → errNoModules） | behavior | UseAllFiles のとき `go list` 経由の module 解決をやめる | guff-build/src/import_path.rs:27-53（`go list` 分岐が無く、常に自前で解決＝結果は go1.27 側と同じ） | not-applicable | S | go/build/build_test.go |
| 14 | go/build/constraint/vers.go minVersion | other | `strings.Cut` の ok を見て早めに -1 を返す。go1.26 も v="" で Atoi が失敗して -1 なので値は同じ | guff-ast/src/constraint.rs:624-636 | not-applicable（差分は挙動中立）。※既存差は下の付記を参照 | S | constraint/vers_test.go |
| 15 | go/build/read.go SkipObjectResolution | other | ImportsOnly の parse で object resolution を省く。性能だけ | — | not-applicable | S | — |
| 16 | internal/pkgbits/version.go V3/V4/V5 + Field | substrate | 版 V3（CompactCompLiterals）・V4（GenericMethods）・V5（PreserveMethodOrder）を追加。**go1.27.x の cmd/compile は V5 を書く**（noder/unified.go:29 `uirVersion = pkgbits.V5`。go1.27.0 は V4） | guff-exportdata/src/pkgbits/version.rs:9-12（NUM_VERSIONS=3）、28-49（Field / introduced）、decoder.rs:39（V3 以上は Decode エラー） | **done (P9-2)** | S | internal/pkgbits/pkgbits_test.go（V3-V5 と Field の対応表） |
| 17 | pkgbits V3 CompactCompLiterals | substrate | 複合リテラル要素の圧縮形式。関数本体（inline 用）だけで使われ、gcimporter は本体を読まない | guff-exportdata/src/ureader.rs（BODY section を読まない。reloc.rs:16 は定数だけ） | not-applicable（#16 で版を受け付ければ足りる） | S | — |
| 18 | gcimporter ureader.go objDictIdx / readerDict / TypeTypeParam | substrate | V4 から dict の先頭に receiver 型引数の数 `nreceivers` を読み、bounds を rtbounds / tbounds に分ける。TypeTypeParam の index は `n < len(rtbounds)` なら rtparams、それ以外は `n-len(rtbounds)` で tparams | guff-exportdata/src/ureader.rs:49-54（bounds が 1 本）、501-535（Len を 1 回だけ読む）、628-631（`dict.tparams[len]`） | **done (P9-2)** | S〜M | go/internal/gcimporter/testdata/genmeth.go（go1.27 で compile した .a） |
| 19 | gcimporter ureader.go ObjFunc | substrate | V4 から ObjFunc の先頭に「generic method か」の Bool（package scope では常に false）。**generic method を使わないパッケージでも必ず読む必要がある** | guff-exportdata/src/ureader.rs:384-393（pos の直後に type_param_names。Bool を読まない） | **done (P9-2)** | S | genmeth.go、加えて任意の go1.27 stdlib .a |
| 20 | gcimporter ureader.go ObjType の methods | substrate | V4 から通常 method の後に generic method の第 2 リスト（`Len` 個の `Reloc(SectionObj)`）。各要素は tempReader + objDictIdx、`Bool`（true）、selector、`typeParamNames(true)`（rtparams）、recv、(V5) index、`typeParamNames(false)`、`signature(recv, rtparams, tparams)` | guff-exportdata/src/ureader.rs:411-415（Len 1 回で method を読むだけ）、856-870（method）、signature は recv・rtparams・tparams を別引数で受けられる（745-751） | **done (P9-2)** | M（guff-types が「recv 型引数 + 自前の型引数」を持つ method を扱えることが前提） | genmeth.go（Option.GetOrDefault / List.Map / List.Zip / *Box 等）、math/rand/v2（*Rand.N） |
| 21 | gcimporter ureader.go objIdx | substrate | 名前に `.` を含む object（package scope に昇格した generic method）を読まずに返す | guff-exportdata/src/ureader.rs:326-341（Stub と vargen suffix だけを除外。`.` の guard 無し → `Option.GetOrDefault` という Func を scope に入れうる） | **done (P9-2)** | S | genmeth.go |
| 22 | gcimporter ureader.go method / V5 index | substrate | V5 で各 method の先頭（generic method は recv の後）に元の index を `Len` で持ち、通常＋generic を index で sort してから AddMethod（go.dev/issue/81188。V4 は通常→generic の順で source order を失う） | guff-exportdata/src/ureader.rs:856-870（index を読まない）、431-433（読んだ順に add_method） | **done (P9-2)** | S | genmeth.go（method 順の比較は gcimporter_test.go TestGenMeth） |
| 23 | gcimporter ureader.go typeParamNames（ローカル捕捉） | substrate | tparams をローカル変数に作ってから dict に置き、bounds は先にローカルへ解決、constraint 設定は later に積む（isGenMeth=false 側の挙動は go1.26 と同じ） | guff-exportdata/src/ureader.rs:821-853（tparams を作って dict.tparams に置いてから bounds を typ_idx で解決し Deferred に積む） | already-matches（isGenMeth=true 側は #18/#20 に含む） | S | — |
| 24 | gcimporter ureader.go newAliasTypeName | default-change | GODEBUG gotypesalias を見ず、常に *types.Alias を作る | guff-exportdata/src/ureader.rs:359-374（常に new_alias） | already-matches | S | — |
| 25 | internal/pkgbits/syncmarker_string.go | other | stringer の生成形の変更だけ | guff-exportdata/src/pkgbits/sync.rs | not-applicable | S | — |
| 26 | （観測）版未対応時の失敗の見え方 | other | 上流（x/tools v0.52 の internal/gcimporter/ureader.go も V5 対応済み）は読めるので失敗しない。guff は V5 で Decode エラーになり、GUFF_DEP_SOURCE=0 では **0 件で静かに終わる**（計測済み）。#16-#22 が入れば消えるが、未知の版で黙らない経路（typecheck エラーとして出す / source にフォールバック）が必要か要確認 | guff-exportdata/src/pkgbits/decoder.rs:39-43 → guff-packages/src/typecheck.rs:507,542 の Err 処理（握りつぶし箇所は未特定） | **P9-2 で測定**: V5 が読めるようになり go1.27 の小 fixture は一致。ただし `GUFF_DEP_SOURCE=0` 経路は **1.27 と無関係にもとから壊れている**（PR P9-1 より前の main d685ce5d・go1.26.5 でも gin の 4 package が ill-typed、pr tier は helm 46 / k9s 33 + panic 3 / cobra 2）。既定経路ではないので Phase 9 の外。未知の版で黙る件も未対応 | S | — |

###### 依存順序

1. **#16（pkgbits 版定数）→ #19（ObjFunc の Bool）→ #18（dict の nreceivers）→ #22（V5 index）** は形式を読み進めるための必須分。generic method を**使わない**パッケージでも V4 以降は ObjFunc の Bool・dict の nreceivers・ObjType の第 2 リストの `Len`（0 でも読む）・method の index を読まないとストリームがずれる。なので #16 だけ先に入れて版チェックを緩めるのは**不可**（誤デコードか panic になる）。#16・#18・#19・#22 と #20 の「Len を読んで 0 件ならスキップ」までを 1 本に入れる。
2. **guff-types が generic method を表現できること**（Signature が recv 型引数と自前の型引数を同時に持つ、Named.add_method、method set / selection での instantiate）が #20 の中身（実際に generic method を読む部分）と #1 の前提。これは go/types 担当の台帳行。
3. **#1（parser）は guff-types の go/types 側の対応と同じ PR か、その後**。parser だけ通すと、今は parse error で package が丸ごと ill-typed（0 件）になっているのが、checker で別の失敗に変わるだけになる。GUFF_DEP_SOURCE=1（依存を source から型検査）では **math/rand/v2 を import するだけで踏む**ので、優先度は高い。
4. #21 は #20 と同じ PR（V4 の package scope に `T.M` が並ぶので、#20 を入れると同時に必要になる）。
5. printer（#10-#12）は Phase 8 で go1.27 に移植済み。#1 の後は、generic method の整形が guff-ast/src/printer/nodes.rs:480-483（signature が type_params を出す）と 1434-1448（func_decl）でそのまま通るはず。gofmt fixture を 1 本足して確認する。

###### PR 分割案

| PR | 内容 | effort |
|----|------|--------|
| A | exportdata V3-V5 の**形式**: #16 #17 #18 #19 #21 #22、#20 のうち第 2 リストの読み飛ばし（generic method は読むが types 側の受け皿ができるまで捨てる、または B を待つ）、#26 の失敗の見え方。fixture: go1.27.3 で compile した stdlib .a（V5）と genmeth.a。go1.27.0 の V4 も 1 本あると良い | M |
| B | exportdata の generic method 本体（#20）: rtparams + tparams 付き signature を作って add_method、V5 で sort。go/types 側の generic method 対応と同時 | M（types 側を含めると L） |
| C | parser #1 + gofmt / golden fixture（genmeth.go、methods.go 相当、math/rand/v2 の `r.N` を使うコード） | S（parser）。B と同時か直後 |

###### 付記（go1.27 の差分ではないが読んで見つかったもの）

- guff-ast/src/constraint.rs:632 は `strip_prefix("go1.")`、上流は go1.26 / go1.27 とも `strings.Cut(tag, "go1.")`（先頭以外でも一致）。`//go:build xgo1.21` のようなタグで上流は 21、guff は -1。既存の差で、優先度は低い。
- guff の scanner は literal を `from_utf8_lossy` で作るので、不正 UTF-8 を含む文字列で `pos + lit.len()`（parser.rs:1568、scanner.rs:944）が source の長さとずれる。ただしその場合は scanner がエラーを出してファイルが parse error になるので、観測できる差は無いと判断した。
- composite literal の key に promoted field の selector（`T{a.b: 1}`）を書ける件は go/types だけの変更。parser は key を任意の式として読んでいる（guff-ast/src/parser.rs:1774-1787、上流の parseElement と同じ）。なので構文層での作業は無い。interface method の型引数は go1.27 でもエラーのまま（上流 parser.go:1152、guff parser.rs:1187 が一致）。

### 3.2 go-types

###### goroot-go/types（go1.26.5 → go1.27.3）

対象: `goroot-go_types.diff`（6,259 行、非テスト）。差分の約半分は operand の accessor 化（`x.mode` → `x.mode()` など）で、
機械的に正規化して取り除いた残り（2,991 行）を読んで分類した（正規化した diff:
`scratchpad/phase9/types-norm/norm.diff`、
testdata の diff は同じディレクトリの `testdata.diff`）。

前提として押さえておくこと（status の重みに効く）:

- **guff は typecheck エラーを finding として出さない**（COMPAT-HARDENING 2 本目の 7）。そのため「文言だけ」の変更は
  今のところ出力に現れない。観測に効くのは (a) guff だけがエラーを出して `ill_typed` → パッケージの解析を
  **黙って skip**（runner/action.rs:503）、(b) 上流だけがエラーを出す（guff は上流が typecheck しか出さないパッケージで
  finding を出す）、(c) Info マップと型の中身、の 3 つ。各行で (a)/(b)/(c)/文言のどれに当たるかを書いた。
- guff の型エラー文言は、もともと上流と一致していない（例: `assignment` は `cannot use {型} value as {型} value in {ctx}`、
  check_assign.rs:351。上流は operand 文字列）。文言の行は「上流の文言に寄せる時に一緒に直す」扱い。
- 動作の確認は scratchpad の probe で行った。go/types を両 toolchain でビルドした小さなドライバ
  （`.../phase9/probe/tc/tc6`, `tc7`）で上流の答えを取り、HEAD 直前の `target/release/guff`（10/31 00:13 ビルド）を
  `GUFF_DEBUG_ILL_TYPED=1` 付きで probe module（`.../phase9/probe/gp/`）に流して guff の `ill_typed` 判定を見た。

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | operand.go 他ほぼ全ファイル（refactor 一括） | refactor | operand の `mode_`/`typ_` 化と `mode()`/`typ()`/`isValid()`/`invalidate()`。`int64(len(constant.StringVal(v)))` → `constant.StringLen(v)`（builtins/index）。`exprWithHint` を `genericExpr(x, e, hint)` に統合（挙動の部分は #14）。universe の error/comparable を `NewNamed` で作るよう変更、Func のフィールド並べ替え、gotype.go の `SkipObjectResolution`、コメントと typo。型・文言・Info・method set・typestring の**どれも変わらない** | operand.rs（pub フィールドのまま）, builtins.rs, index.rs | not-applicable | S | — |
| 2 | check.go / decl.go / typexpr.go / scope.go / universe.go / typestring.go / api.go / resolver.go / object.go | behavior | GODEBUG `gotypesalias` を撤去。Alias を常に作り、`Universe` の `any` は `&emptyInterface` を指す Alias 1 本だけになる。typestring の「universeAny の underlying と同じポインタなら `any` と書く」特例も削除（**観測差**: `Unalias(any)` を印字すると 1.26 は `any`、1.27 は `interface{}`）。`invalid use of type alias … (go.dev/issue/50729)` と `generic type alias requires GODEBUG=gotypesalias=1` は消滅 | universe.rs:201-206（常に `new_alias`）, typestring.rs:258-288（write_interface に any の特例が無い）, typexpr.rs（broken alias の経路が無い） | already-matches | S | check/cycles5.go, fixedbugs/issue50779.go, spec/typeAliases1.23.go（a/b は削除） |
| 3 | api.go / typelists.go | new-api | `Instance.String()`, `TypeList.String()`, `TypeParamList.String()` を追加。表示用の API だけ | — | not-applicable | S | — |
| 4 | hash.go / trie.go（Hasher 部分） | new-api | `types.Hasher` / `HasherIgnoreTags`（`maphash.Hasher[Type]`、`Identical` / `IdenticalIgnoreTags` と整合）。go1.27 の std で使っているのは stdlib manifest だけ。x/tools が使い始めたら xtools 台帳で扱う | guff-types/src/hash.rs は FxHash の別物で無関係 | not-applicable | S | — |
| 5 | resolver.go / signature.go / interface.go | language | **generic method の宣言を受理する**。`funcType` の `methods cannot have type parameters`（InvalidMethodTypeParams）を削除し、resolver に `generic method requires go1.27 or later` を追加。go1.18 の `type parameter` 版検査は init/main 以外の通常関数だけに移動。`NewSignatureType` の「recv と tparams を同時に持てない」panic を削除。interface メソッドの文言は `interface methods cannot have type parameters` に変更 | signature_check.rs:78-85（まだ InvalidMethodTypeParams を出す）、resolver.rs:595-660（版検査は 1.18 分も含めて無い）。**前提**: guff-ast/src/parser.rs:2836-2845 が `method must have no type parameters` を出して tparams を捨てる → probe gp/genmeth2 では **a.go がパースエラーとしてファイルごと黙って落ち**、b.go だけが解析された（(a) より悪い）。interface メソッドの tparams は parser.rs:1175-1200 で捨てているので interface.go の文言は届かない | **done (P9-1)**：InvalidMethodTypeParams 削除、resolver に go1.18 / go1.27 の版検査（init / main の InvalidInitDecl は未移植のまま） | S | fixedbugs/issue50427a.go, issue50427b.go（issue50427.go は削除）, check/typeparams.go, spec/methods.go |
| 6 | call.go（selector）/ selection.go | language | generic method の **method value / method expression の署名が自分の type parameters を保持する**ので、`x.m[int]` / `x.m(2)`（推論）/ `T.m[int]` / `var f func(T,int) int = T.m`（逆推論）/ 昇格メソッド経由のどれも通る | call.rs:1349-1357（method expr の `new_signature_type(…, &[], &[], …)`）、call.rs:1420-1428（method value も同様）、call.rs:1559-1560（`instantiated_method` の複製も tparams を捨てる）。`is_generic_func_value`（call.rs:292-304）は tparams を見るので、残れば `func_inst` の経路にそのまま乗る | **done (P9-1)** | M | spec/methods.go |
| 7 | subst.go / named.go（expandMethod）/ instantiate.go / mono.go 呼び出し | language | generic type に付いた generic method: receiver を代入するとき、method 型引数の bound が receiver 型引数を参照できる（`func (G[F]) m3[P *F]`）ので、**fresh な type parameter を作って bound を代入**し、params/results をそれに付け替える（受け手の instance ごとに別の tparam）。fresh tparam は `mono.recordCanon` で元に結ぶ。`expandMethod` は `rparams = nil` | subst.rs:241-266（`subst_signature` は tparams も rparams も**常に**捨てる。1.26 の上流は保持していた）、call.rs:1464-1504（`method_sig_for_recv`）、mono.rs:38-39（canon map は作るが未配線） | **done (P9-1)**：fresh tparam と bound の代入。mono の recordCanon は未配線（subst が checker を持たない） | M | spec/methods.go（`G[int]{}.m3[*string]` の `*string does not satisfy *int`、m4 の sibling 呼び出し） |
| 8 | named.go expandMethod | behavior | 「`rbase == t` なら origin のメソッドをそのまま返す」近道を削除。generic method の本体内で `r.m()` を呼ぶと、`Info.Uses` / `Selections` には**宣言した Func ではなく複製**（`Origin()` が宣言、rparams 0）が入る。probe（`.../probe/tc2`）で 1.26 は `use==decl: true`、1.27 は `false` を確認。(c) に効く。再帰検出のように `Uses[id] == decl` と比べている linter が影響を受け得る | call.rs:1527-1533（instance なら必ず複製を作る）、signature_check.rs:291-297（receiver `T[P]` は `instantiate` した instance）→ guff は**もともと 1.27 と同じ**（1.26 とずれていた） | already-matches | S | — |
| 9 | recording.go / signature.go `recvold` / selection.go | info | generic method の method value / expression の `Info.Instances[ident].Type` は、receiver を戻した Signature で記録する（go.dev/issue/79657）。method expression は params[0] を recv に戻す（`methodExprSentinel`） | recording.rs:171-189（`record_instance` は受け取った typ をそのまま入れる）、call.rs:518 / 996 | needs-port | S | —（api_test.go 側） |
| 10 | unify.go | language | interface 推論で、**generic method は interface メソッドを満たさない**（`ym.Signature().TypeParams() != nil` なら失敗）。でないと method 自身の tparam が推論変数に流れ込む | unify.rs:539-559（`nify(xm.typ, ym.typ)` だけで tparams の検査が無い。今は #6/#7 で tparams が落ちるので表面化しない） | **done (P9-1)** | S | spec/methods.go（`need(T{})` の `cannot infer X`） |
| 11 | unify.go `newUnifier(check, …)` + `objDecl(ym)` | behavior | interface 推論の前にメソッドを型検査する（go.dev/issue/77905）。メソッド宣言が呼び出しより後ろにあると 1.26 は `cannot infer T` で落ちていた。1.27 は通る | call.rs:849-851（推論の前に `prepare_method_set` → `ensure_method_sigs`）。probe gp/ifaceinfer は guff で ill_typed にならない（tc6 は失敗、tc7 は通る） | already-matches | S | fixedbugs/issue77905.go |
| 12 | literals.go / lookup.go（`fieldPath`、`fieldIndex` 削除）/ trie.go | language | **struct literal のキーに昇格フィールドを書ける**（`Foo{Baz: 1}`）。解決は `lookupFieldOrMethod` で行う。新しいエラー: `use of promoted field Bar.Baz in struct literal of type Foo requires go1.27 or later`、`%s is not a field`、`invalid implicit pointer indirection to reach %s`、trie での重なり検出（`cannot specify promoted field x and enclosing embedded field C` / `cannot specify embedded field B and enclosed promoted field b`）。unknown field の候補（`but does have x`）も昇格フィールドから探す | literals.rs:243-317（直下のフィールドしか見ない `field_index`）。probe gp/promoted が `unknown field Baz in struct literal of type Foo` で **ill_typed → パッケージ skip**（(a)） | **done (P9-3)**：types と guff-ssa の comp_lit（昇格キーは implicit selection を経由） | M | spec/structLits.go, fixedbugs/issue79832.go, check/expr3.go（`T1{c: 0}` がエラーでなくなる） |
| 13 | literals.go | message | 位置指定の struct literal で値が足りないとき、他パッケージの非公開フィールドがあれば `(type has unexported fields - use key:value pairs)` を付ける | literals.rs:353-361 | **done (P9-3)** | S | — |
| 14 | assignments.go / expr.go `newTarget` / literals.go / index.go / stmt.go / call.go（conversion） | language | **代入・変換の全文脈で関数型からの型推論**。`assignment()` 自身が `nonGeneric(newTarget(T, context), x)` を呼ぶようになり、struct/array/slice/map literal の要素・map のキー（`x[f] = true`）・send 文・変換 `F(g)` で generic 関数の型引数が推論される。`newTarget` は `commonUnder` を使うので、func の core type を持つ type parameter（`[F func(int)]`）も target になる。文言は `cannot use generic function g without instantiation`（末尾の ` in %s` が消える） | check_assign.rs:281-357（`assignment` に non_generic が無い）、expr.rs:63-67（`new_target` は `underlying` を見るので type parameter は target にならない）、call.rs:150-190（変換は target なし）、literals.rs:260 / 326 / 389 / 422 / 484、index.rs:189 / 376、stmt.rs:1014-1018。probe gp/funcinfer で `[]F{g}`・struct literal・map literal・`F(g)` の 4 件すべてが **ill_typed**（(a)） | **done (P9-3)**：`assignment` で non_generic（target が generic な署名なら除外）、変換に target、`new_target` は common_under。既存テストの「struct literal は拒否」を go1.27.3 の実測（go 1.21/1.26/1.27 とも受理）に合わせた | M | fixedbugs/issue77245.go, issue80042.go |
| 15 | assignments.go / expr.go（`genericExprList` の dummy 式、`nth`） | message | 左辺が 2 個以上の代入は context を `multiple assignment` にする。多値の各要素の operand 名を `2nd function result` / `ok value of (comma, ok) expression` / `err value of (comma, err) expression` にする。probe で 1.26/1.27 の文言差を確認 | check_assign.rs:573 / 593 / 648-652（context は常に `assignment`）、check_assign.rs:722-760（`eval_multi` は元の式を使い回す）。guff の文言は operand を印字しない形（check_assign.rs:351）なので、効くのは context だけ | needs-port | S | fixedbugs/issue78884.go, check/issues0.go, spec/range.go |
| 16 | call.go（selector の Var/Func 分岐の組み替え） | message | 型式に対するフィールド選択（`S.x`）は `operand for field selector x must be value of type S` で、位置は `e.X`（1.26 は Sel の位置に `S.x undefined (type S has no method x)`）。ほかは Func の `objDecl`/`addDeclDep` を前に出しただけ | call.rs:1308-1322 | needs-port | S | fixedbugs/issue6814.go, check/expr3.go, check/issues0.go |
| 17 | cycles.go `isComplete`（`finiteSize` / `pendingType` を置き換え）/ expr.go / call.go / index.go / literals.go | behavior | 不完全な型（宣言中で underlying の無い Named/Alias、alias の連鎖を含む）の使用を各所で止めて cycle を報告する: 変換、呼び出しの結果、selector（pointer の base も）、`<-`、型アサーション、`*p`、index（pointer の base、map の elem）、slice、composite literal の base。不正な再帰型を含むコードにしか効かない（invalid → invalid） | 該当なし（guff は 1.26 の `finiteSize` / `pendingType` も持っていない） | needs-port | M | check/cycles6.go（T14）, fixedbugs/issue79265.go |
| 18 | builtins.go `hasVarSize` | behavior | Checker のメソッドになり、objPath で cycle を検出して `cycleError` を出し true を返す（→ `unsafe.Sizeof` が定数でなくなり、`is not constant` / `must be constant` が増える）。結果を `Named.varSize` にキャッシュする。underlying ではなく RHS をたどる。`simd._simd` は可変サイズ扱い。`TypeParam` の分岐を明示した（意味は同じ）。正しいコードで差が出るのは simd だけ | builtins.rs:812-844（1.26 のアルゴリズムのまま） | needs-port | S | fixedbugs/issue52915.go, issue75918.go, issue76478.go, issue77382.go |
| 19 | decl.go `cycleError` | message | cycle の名前に型引数を付ける（`invalid recursive type: T[int] refers to itself`。probe で確認） | cycles.rs:133-155（書式も簡略版）, decl.rs:667-679 | needs-port | S | fixedbugs/issue52915.go |
| 20 | const.go `overflow` | behavior | 型なし文字列定数の長さが 2e9 を超えたらエラー（go.dev/issue/78346）。`constant.StringLen` を使う | check_expr_const.rs:254-295、guff-constant/src/value.rs:241（`string_val` は全体を実体化する。StringLen は無い） | needs-port | S | fixedbugs/issue78346.go（`//go:build ignore` なので上流のテストでも実行されない） |
| 21 | builtins.go copy | behavior | 第 1 引数は特別形（`[]byte ← string`）でも slice でなければならない。`copy(nil, []byte{})` は `invalid copy: argument must be a slice; have untyped nil`（go.dev/issue/79687） | builtins.rs:1280-1331（dst の検査が `!special` の中だけ）。probe gp/copynil: guff は通し、tc7 はエラー（(b)） | needs-port | S | fixedbugs/issue79687.go |
| 22 | range.go | behavior | range-over-func の yield が可変長なら `yield func of type %s cannot be variadic`（go.dev/issue/78483） | stmt.rs:393-433（variadic を見ない。1.26 分の cause 文言も無い）。probe gp/yieldvar: guff は通し、tc7 はエラー（(b)） | needs-port | S | fixedbugs/issue78483.go |
| 23 | stmt.go RangeStmt | other | 変数の無い `for range x {}` で `s.TokPos` が無効なとき `s.For` を使う（inNode の assert で落ちる crash の修正） | stmt.rs:1195-1230（noNewVarPos を使わない） | not-applicable | S | — |
| 24 | lookup.go / object.go `nointerface` | other | `missingMethod` に nointerface 状態と `(m method is marked 'nointerface')` を追加。go/types は pragma を見ないので立たない（types2 用） | — | not-applicable | S | — |
| 25 | instantiate.go / lookup.go `interfacePtrError` | message | implements の詳細で、type parameter へのポインタを `type *P is pointer to type parameter, not type parameter` と書く（1.26 は `pointer to interface`。probe で確認） | check_lookup.rs:300-309（isInterfacePtr の詳細そのものが無い）、call.rs:1283-1284 | needs-port | S | fixedbugs/issue79883.go |
| 26 | instantiate.go `Instantiate`（公開 API） | new-api | 引数の誤りで明示的に panic / error を返す（`cannot instantiate non-generic …`、`cannot instantiate %s: got %d type arguments but have %d type parameters`）。Signature には generic method 用の assert | instantiate.rs:39-49（validate なし・soft-return） | not-applicable | S | — |
| 27 | named.go | substrate | 遅延 loader（export data）が delayed 関数より先に `hasUnder` まで状態を進める（go.dev/issue/80258 の deadlock）。`allowNilUnderlying` を `allowNilRHS` に統合、`resolveUnderlying` の assert を整理、`finite` → `varSize` | named.rs:8-20（遅延 loader も状態機械も持たない） | not-applicable | S | — |
| 28 | testdata のみ（issue48703 → issue80172 へ移動） | other | `func (_ List[P]) m() (_ List[List[P]])` の instantiation cycle。**1.26 も 1.27 も報告する**（probe）ので 1.27 の差分ではない。ただし guff は通す（probe gp/instcycle、(b)）。mono.rs:38-39 の canon 未配線とは別件の既存ギャップ | mono.rs | not-applicable | — | fixedbugs/issue80172.go |

集計: needs-port 17 / already-matches 3 / not-applicable 8 / unsure 0（計 28 行）。

反証で確かめたこと:

- #2: guff の中で `"any"` を書くのは universe.rs:205 だけ。typestring に特例は無い。builtin の記録で guff が `universe_any`（Alias）を使い（builtins.rs:463-499）`func(any)` と印字するのは 1.26 からの既存差で、1.27 の差分ではない。
- #8: 複製を作る条件は「receiver が instance かどうか」だけ（call.rs:1529-1530）。method 本体内の receiver `T[P]` は collect_recv が `instantiate` で作った instance（signature_check.rs:291-297）なので近道は通らない。複製の rparams は `new_signature_type` が None にし、origin は `set_origin` で宣言を指す。
- #11: guff の unify は `ym.typ == None` なら失敗する（unify.rs:545-559）。それでも通るのは、推論の前に call.rs:849-851 で引数型のメソッド署名を揃えているから。この経路が無い入口（`infer` の外の interface 推論）が見つかれば unsure に戻す。

###### 依存順

1. **guff-ast parser**（go_parser 台帳。この台帳の外）: `parse_func_decl` が method の tparams を捨ててエラーにするのをやめる（parser.rs:2836-2845）。これが無いと型検査には届かず、ファイルが黙って消える。
2. #5（受理と版検査）→ #6（method value / expression の tparams 保持）→ #7（receiver 代入での fresh tparam）。#6 と #7 は `instantiated_method` / `method_sig_for_recv` を共有するので、1 本にまとめるのが楽。
3. #10（unify）と #9（Instances）は #6/#7 の後でないと意味が無い。
4. #14（関数型推論）は独立。#12（struct literal）も独立。どちらも (a) を消す。
5. #21/#22 は独立で小さい（(b) を消す）。
6. 文言の行（#13/#15/#16/#19/#25）は「typecheck を finding にする」作業と一緒にやるのが自然。単独では出力が変わらない。
7. #17/#18/#20 は不正コード専用で、優先度は最低。

###### PR 分割の提案

| PR | 行 | 内容 | effort |
|----|----|------|--------|
| T1 | （parser）+ #5 + #6 + #7 | generic method を最小限通す。parser の受理、InvalidMethodTypeParams の削除と `generic method requires go1.27` の版検査、method value / expression と `instantiated_method` で tparams を残す（`new_generic_signature_type` を使う）、`subst_signature` で tparams/rparams を保持し、receiver 代入のときは fresh tparam を作る。mono canon（#7 の recordCanon）はこの PR では任意 | M〜L |
| T2 | #10 + #9 | generic method が interface を満たさないこと（推論側）と、Instances の receiver 復元 | S |
| T3 | #14 | `assignment` に `non_generic(new_target(T))`、`new_target` を `common_under` に、変換に target を渡す。guff の `expr` はもともと generic 値を弾かないので、要素ごとに `genericExpr` へ置き換える必要は無く、`assignment` 1 箇所と変換で全文脈が揃う | M |
| T4 | #12 + #13 | struct literal の昇格フィールド（`lookup_field_or_method` で解決、版検査、indirect の拒否、trie での重なり検出、unknown field の候補）と不足値のヒント | M |
| T5 | #21 + #22 | copy の dst 検査を前に出す、yield の variadic を拒否 | S |
| T6 | #15 + #16 + #19 + #25 | 文言（typecheck finding 化とまとめるなら後回しでよい） | S |
| T7 | #17 + #18 + #20 | isComplete / hasVarSize の書き直し / 文字列定数の長さ | M |

###### Go 1.27 の generic method を使うパッケージを guff が黙って落とさなくなる最小セット

1. **parser**（guff-ast/src/parser.rs:2836-2845）: method の `[P any]` を `FuncDecl.ty.type_params` に残し、エラーにしない。今はここでファイルが落ちる（probe gp/genmeth2: `unusedInA` が報告されず `unusedInB` だけ出る）。
2. **#5**: signature_check.rs:78-85 の `methods cannot have type parameters` を消す（残すとパッケージ全体が `ill_typed` になり skip される）。go1.27 の版検査も足す。上流は `-lang < go1.27` ではエラーにするので、ここで受理しすぎないように。
3. **#6**: call.rs:1349-1357 / 1420-1428 / 1559-1560 で method の tparams を落とさない。落とすと `x.m(2)` は非 generic な署名として `P` 型の引数と照合され、`x.m[int]` は `is_generic_func_value` が偽になって index 式扱いになる。どちらもエラーになり `ill_typed` になる見込み（コードを読んだ推定。parser を直した後の probe はしていない）。
4. **#7**: subst.rs:241-266 で generic type 上の generic method の tparams を保持する（instance ごとに fresh tparam）。落とすと `G[int]{}.m3(&i)` のような呼び出しが壊れる。非 generic な receiver だけなら 1〜3 で足りる。

型検査が通った後の SSA（guff-ssa の generic method 対応）は x/tools 台帳の範囲。#9/#10 は正しく拒否するため・Info を正確にするためのもので、「落とさない」ための最小セットには入らない。

### 3.3 xtools

#### x/tools v0.50.0 → v0.52.0（golangci-lint 2.14.0 → 2.15.0）

diff: `diffs/xtools.diff`（142 ファイル、*.md / go.sum 除外）。読み取り専用で作成。「guff の場所」は `crates/` を省略。

**前提の訂正（依頼文の想定と diff が違ったもの）**

- **新しい modernize analyzer は無い**。`atomictypes` / `embedlit` / `slicesbackward` / `unsafefuncs` / `waitgroupgo` はどれも v0.50 に既にあり、
  `fmtappendf` も v0.50 で Suite から外れている。`modernize.go` の差分は import の並べ替えだけで、`Suite` は v0.50 と同じ。
- **`internal/stdlib/manifest.go` と `internal/versions` は 0 行**。go1.27 シンボルは v0.50 で入り済み（PR 14t / 14u で転記済み）。再転記は不要。
- **printf.go 本体も 0 行**。go1.27 の `%w` / `%d` ポインタ規則は v0.50 からあるもので、v0.52 の差分は test の分離（`TestW`）と testdata の build tag だけ。
- go/ssa の大きい差分は**ジェネリックメソッドではない**。中身は `compLit` の storebuf 撤去（命令の順序が変わる）、単一 case の select、性能まわり。ジェネリックメソッド関係は canonizer の 1 行と `MethodValue` の nil 返しだけ。

**実測**（`phase9/bin/golangci-lint` 2.15.0 / Homebrew 2.12.2 / `phase9/bin/guff` を scratchpad の最小 module で比べた）:
modernize の 6 形、stringintconv の `--fix`、goimports の `//go:embed`、import cycle の typecheck は下表の「実測」どおり。
guff の出力は `phase9/{mz,mz2,nx,sic,emb,cyc,cyc2}/` に残してある。

##### 概要

| グループ | needs-port | already-matches | not-applicable | unsure | 計 |
|---|---:|---:|---:|---:|---:|
| xtools-modernize | 6 | 3 | 2 | 0 | **11** |
| xtools-passes | 3 | 2 | 3 | 0 | **8** |
| xtools-substrate | 5 | 2 | 7 | 0 | **14** |
| **合計** | 14 | 7 | 12 | 0 | **33** |

needs-port 14 のうち 3 行（modernize #7 embedlit、passes #9 printf go1.27、substrate #13 export data の `'i'` 形式）は
**Phase 8 からの持ち越し**で、v0.52 で増えたのはそれぞれの小さな追加分だけ。v0.52 で新しく出た作業は 11 行。

---

###### xtools-modernize

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | modernize/slicesbackward.go | behavior | 3 つの guard を追加：(a) `name := s[i]` を値変数として採るのは**ループ本体の先頭文のときだけ**（入れ子ループ・func lit・if・goto ラベル後は不可）。(b) `s[i]` があり、s に副作用がありうる（`!NoEffects`）なら報告しない。(c) 本体が i も `s[i]` も読まないなら報告しない | guff-style/src/modernize.rs:3995-4040 `check_slicesbackward`（`first_assign` は入れ子でも採る。(b)(c) の guard が無い） | needs-port（**実測**：空の本体と `next()[i]` の 2 形で guff だけ報告する。`declInBlock` の fix は guff が `v := items[i]` を消して `v` を使い、上流は `item` を使って `v := item` を残す） | S | slicesbackward/slicesbackward.go{,.golden}（indexUnused, emptyBody, declInNestedLoop, declInFuncLit, declInBlock, declAfterLabel, labeledDecl, sliceExprWithSideEffects{,NotIndexed}） |
| 2 | modernize/stditerators.go | behavior | 本体に `x.At(i)` が 1 つも無ければ報告しない（新しいループ変数が未使用になり fix がコンパイルできない）。for と range の両パターンで共有している | modernize.rs:8118 `stditerators_at_edits`（使用 0 のとき `Some(vec![])` を返し、8192 / 8293 の両パターンが報告する） | needs-port（**実測**：`for i := 0; i < t.NumFields(); i++ { print() }` で guff だけ報告） | S | stditerators/stditerators.go{,.golden}（NumField の 2 行） |
| 3 | modernize/stringscut.go iType | behavior | `var i int = strings.Index(...)` の fix で型 `int` を削除する（fix のみ。診断は v0.50 と同じ） | modernize.rs:6150-6170（ValueSpec の `Type` を見ない） | needs-port（**実測**：guff の fix は `var found int = strings.Contains(...)` になり、コンパイルできない） | S | stringscut/stringscut.go{,.golden}（idx_var_typed_*）、stringscut_go127.go{,.golden}（lastindex_var_typed_*、golden では測れない） |
| 4 | modernize/stringscut.go byte 変数 | behavior | `IndexByte(s, b)` で b が変数のとき `string(b)` ではなく `string([]byte{b})`（0x80 以上が 2 バイトの UTF-8 になるのを避ける） | modernize.rs:6193 `"string("` | needs-port（**実測**：`strings.Cut(s, string(b))` と出す。上流は `string([]byte{b})`） | S | stringscut/stringscut.go{,.golden}（strings_byte_var_nonascii{,_contains}）、stringscut_go127.go |
| 5 | modernize/stringscut.go byte 定数 | behavior | 定数 byte を `string([]byte{byte(val)})` で quote するので `0xff` は `"\xff"`（v0.50 は `"ÿ"`） | modernize.rs:6199 `quote_bytes(&[v as u8])`（不正な UTF-8 を `\x` で出す） | already-matches（**実測**で一致：`strings.Cut(s, "\xff")`。guff は v0.50 とは食い違っていて、v0.52 と偶然一致していた） | S | stringscut/stringscut.go{,.golden}（strings_byte_nonascii*, bytes_byte_nonascii） |
| 6 | modernize/newexpr.go | behavior | `//go:fix inline` が既にあるかを `ast.ParseDirective` で見る（`//go:fix inline // 理由` は**別物**になり、2 行目を足す） | modernize.rs:6279 `has_go_fix_inline`、6617 | needs-port（**実測**で既存の欠陥が出た：解析 AST はコメントを持たないので `fd.doc` が常に None。**`//go:fix inline` が既にあっても 2 行目を足す**（上流は足さない）。v0.50 から壊れていた。直すときは guff-ast/src/directive.rs:41 `parse_directive` で判定する。現状のゆるい部分一致 `starts_with("go:fix") && contains("inline")` も上流と違う） | S | newexpr/newexpr.go{,.golden}（自作で `// keep` 付きと無しの 2 形を足す） |
| 7 | modernize/embedlit.go | new-check | （Phase 8 #6 の持ち越し）v0.52 は fix の guard を 2 つ足しただけ：Lbrace / Rbrace が不正・範囲外なら fix を出さない（#81590）、`startPos < endPos <= closingPos` | 無し（modernize.rs の `enabled(&options, …)` に embedlit が無い） | needs-port（持ち越し。Go 1.27 ファイル限定なので golden では測れない） | L | embedlit*.go{,.golden} |
| 8 | modernize/bloop.go | behavior | timer 呼び出しの削除を `refactor.DeleteStmt` にした（行ごと、同じ行のコメントも消す） | guff-lint/src/settings.rs:4136 SUITE_EXTRA_OFF `"bloop"`（Suite 外。上流の Suite でもコメントアウトされたまま） | not-applicable | S | bloop/bloop_test.go{,.golden} |
| 9 | internal/refactor/delete.go safeLineOf | behavior | DeleteStmt / DeleteDecl の行判定が `//line` を無視する | guff-analysis/src/refactor.rs:279 `delete_with_line`（ソースのバイト列で行を判定するので `//line` はもともと効かない） | already-matches | S | refactor/delete_test.go TestDeleteDeclLineDirective |
| 10 | modernize/{reflect,reflecttypeassert,stringsbuilder,omitzero,modernize,doc}.go | other | `astutil.UnparenEnclosingCursor` → `cursorutil.UnparenEnclosing`（同じ実装を移しただけ）、`sync.OnceValue` の型引数、import の順序、doc の追記 | — | not-applicable（挙動の変化なし。cursorutil/util.go と旧 astutil/cursor.go を突き合わせて同じ） | — | — |
| 11 | modernize.go Suite | default-change | Suite の構成は v0.50 と同じ（新しい analyzer は無い） | settings.rs:4134-4138 SUITE_EXTRA_OFF（slicesdelete / bloop / fmtappendf） | already-matches（欠けているのは #7 embedlit だけ） | — | — |

###### xtools-passes（govet）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | passes/stringintconv/string.go → internal/analysis/intconv/intconv.go | behavior | 診断（文言・位置）は同じで、**fix を作り直した**。Fix 1（挙動を保つ）は `string(rune(x))` を、32 bit に収まる型か定数のときだけ出す。それより広い型は `fmt.Sprintf("%c", x)` にする（型パラメータと untyped 定数では出さない）。Fix 2（挙動が変わる）は `fmt.Sprint` から `fmt.Sprintf("%d", x)` に変わり、メッセージも変わった。挙動を保つ fix が無いときと、rune に変換できないときは fix を一切出さない。`string(x)` は呼び出し全体を置き換える | guff-govet/src/stringintconv.rs:185-245（v0.50 の 2 fix：`fmt.Sprint` と `rune(`） | needs-port（**実測、--fix**：int16 の `string(j)` は 2.15 が `fmt.Sprintf("%d", rune(j))`、guff が `fmt.Sprint(rune(j))`。int / int64 では 2 つの fix が同じ範囲を書くので、2.15 は "conflicting edits from govet and govet" を出して**そのファイルの govet の変更を全部捨てる**。guff は書いてしまう。golangci が全 fix の edits を合成する前提（stringintconv.rs:5-9 の doc）は残るが、合成の結果が変わる） | M | stringintconv/testdata/src/fix/{fix,fixdot,fixnamed,fixtarget,fixwide}.go{,.golden}、preserve_test.go（golden は単独適用の結果。golangci の合成結果は実測で作る） |
| 2 | suite/fix/fix.go、stringintconv/doc.go・testdata の build tag、printf_test.go TestW・testdata の build tag | other | cmd/fix の suite に stringintconv を入れた。go1.28 で `string(int)` がエラーになる件の doc。test の版の固定（`a/a.go` に `//go:build go1.25`、`nonconst.go` に `go1.23`、stringintconv の testdata に `go1.21`） | — | not-applicable（cmd/fix と test だけ。**ただし** `govet-printf-upstream` の golden を v0.52 の testdata で取り直すと、`//go:build go1.25` がファイルの版を下げるので、版で切り替わる検査の結果が変わる。取り直すなら build tag ごと転記する） | — | — |
| 3 | internal/astutil/comment.go Directives → ast.ParseDirective（inline の gofixdirective） | behavior | args の後ろの `// …` を切らなくなった。`//go:fix inline // 理由` は inline 指示**ではなくなる** | guff-govet/src/inline.rs:115-121 `has_fix_inline` → guff-ast/src/directive.rs:41 `parse_directive`（go/ast の移植で `//` を切らない） | already-matches（guff は v0.50 とは食い違っていて、v0.52 と一致した） | S | — |
| 4 | 同上（区切りの空白） | behavior | name の切り出しが `" "` から任意の空白になった。`//go:fix\tinline` が inline 指示に**なる** | inline.rs:232 / 282 / 425 / 654 / 879 / 953 の `memmem::find(src, b"go:fix inline")` 前段フィルタ（タブ区切りのファイルはここで落ちる） | needs-port（端のケース。前段フィルタを `go:fix` だけにするか、`parse_directive` を通す） | S | — |
| 5 | passes/nilness hasCgoUnsafeArgs（同じ Directives の変更） | behavior | `//go:cgo_unsafe_args\t…` が一致するようになった | guff-govet/src/nilness.rs:192-197（`rest.is_empty() \|\| starts_with([' ', '\t'])`） | already-matches | S | — |
| 6 | passes/inline/inline.go、internal/refactor/inline/*（inline.go +371/-218、void-tail.txtar ほか） | behavior | inliner の改修：void な末尾呼び出し、binding decl を `x := h(1)` に平らにする、版チェックをアクセス可能性チェックの後に回す、#80834 の guard。analyzer 側は `Call: cur` に変えただけ | inline.rs:749-800（関数 inliner は無い。ioutil の 6 関数は「本体が転送 1 行」という形だけで判定していて、fix の edits は空） | not-applicable（Phase 8 substrate #12 と同じ扱い。ioutil の転送本体は void でも binding でもないので、変わった経路を通らない。関数 inliner が無いという既存の大きな未移植はそのまま） | L | inline/testdata/src/binding_true/a.go.golden、refactor/inline/testdata/void-tail.txtar |
| 7 | passes/{httpmux,httpresponse,slog,unmarshal}、passes/inline withinTestOf | other | `typesinternal.ReceiverNamed(recv)` → `RecvBase(fn)` / `IsPointerToNamed`。中身は同じ（Unalias → ポインタを剥がす → `*Named`） | guff-govet/src/{httpresponse,slog,unmarshal}.rs（httpmux は guff に無い。既定で無効の analyzer で、v0.52 とは関係ない既存の欠け） | not-applicable（recv.go と isnamed.go の v0.52 版で同じ意味だと確認した） | — | — |
| 8 | printf.go okPrintfArg go1.27（`%w` に `*E`、`%d` にポインタ） | behavior | （Phase 8 #5 の持ち越し）v0.52 で printf.go は 0 行。test を `TestW`（go1.27 の go コマンドが要る）に分けただけ | guff-govet/src/printf.rs:10, 56, 1040（「PR 15」として未移植と明記） | needs-port（持ち越し） | M | printf/testdata/src/w/w.go、issue62595/a_go127.go |

###### xtools-substrate

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | go/ssa/builder.go compLit / assign | substrate | `compLit` から `isZero` と storebuf を外した。fresh な Alloc への**フィールド・配列要素の Store が要素の評価と交互に出る**ようになった（v0.50 は全要素を評価した後にまとめて Store していた）。slice と map の最後の Store も即時。memclear の枝は削除（v0.50 でも到達しなかった） | guff-ssa/src/builder/stmt.rs:1450-1700 `comp_lit` / `comp_lit_struct`:1547 / `comp_lit_array_slice`:1630 / `comp_lit_map`:1697、builder/mod.rs:1008-1010（v0.50 の形） | needs-port（`T{A: f(), B: g()}` は v0.50 が `FieldAddr A; f(); FieldAddr B; g(); Store A; Store B`、v0.52 が `…f(); Store A; …g(); Store B`。lift の後も complit の Alloc は FieldAddr があるので残る。Referrers の順序に依存する linter（bodyclose / sqlclosecheck / gosec の taint など）で出る可能性がある。lint で観測できるかは未測定） | M | ssa/interp/testdata/complit_go128.go、coverage.go（interp 用。guff には interp が無いので、ssadump v0.52 とブロック単位で突き合わせる。PR 14w と同じやり方） |
| 2 | go/ssa/builder.go assignSelectRecvStmt | substrate | 単一 case の `select { case v := <-ch: }` は、受信を**先に**評価してから左辺を評価する（v0.50 は通常の assignStmt と同じで左辺が先） | stmt.rs:1061-1066 `select_stmt`（`self.stmt(comm)`） | needs-port（`case a[i()] = <-ch` の呼び出し順と、escape する `v` の Alloc の位置が変わる） | S | — |
| 3 | go/ssa/builder.go expr0 CompositeLit | substrate | 暗黙の `&T{}` を assign から expr0 に移した。go1.28 の型省略 `f({})` も同じ経路を通る | stmt.rs:1425-1436 `assign`（ポインタ型の位置なら `address(e, true)` → Store。v0.52 の expr0 と同じ命令になる） | already-matches（既存の形について。go1.28 の型省略は guff-types が受け付けないので not-applicable） | — | — |
| 4 | go/ssa methods.go / util.go canonizer / generic_methods_test.go | substrate | Go 1.27 ジェネリックメソッド：`MethodValue` がジェネリックメソッドに nil を返す。canonizer はレシーバ付きの Signature を代表にしない（#81602） | guff-ssa/src/canon.rs:47、methods.rs:56-71 | not-applicable（Phase 8 substrate #9 と同じ。レシーバ付きの Signature が canonizer に入るのはジェネリックメソッドの場合だけで、guff-types はそれを受け付けない） | — | generic_methods_test.go TestIssue81602 |
| 5 | go/ssa methods.go methodKey / existingMethod、builder.go waitForSharedFunction、task.go、func.go `fn.info = nil` | other | 割り当ての削減、LookupMethod の高速路、task の transitive フラグのバグ修正（`x` → `u`）、ビルド後に types.Info を解放 | methods.rs:69（`[t, sel_typ]` での判定はジェネリックでないメソッドなら `t` だけと同値） | not-applicable（性能・並行性・メモリだけ。出力は変わらない） | — | methods_test.go TestMethodValueNoAlloc |
| 6 | internal/typesinternal/assignedaddress.go | behavior | 埋め込みフィールドの走査を `ImplicitFieldSelections` に変えた（ポインタを剥がし、struct でなければ止まる） | guff-analysis/src/typesinternal.rs:62-74 | already-matches（直前の `seln.Indirect()` で早期に return するので、走査はポインタに出会わない。どちらの版でも結果は同じ） | — | — |
| 7 | typesinternal recv.go / isnamed.go / types.go / varkind*.go、internal/aliases、go/types/objectpath、typeindex.go、internal/astutil/{util,cursor}.go、internal/cursorutil | other | `ReceiverNamed` → `RecvBase`、VarKind は go/types の本物を使う、`aliases.New` → `types.NewAlias`、cursor ヘルパの移動、objectpath の依存制限 | — | not-applicable（どれも同じ意味の書き換え） | — | — |
| 8 | go/packages/golist.go import cycle | message | `import cycle not allowed in test` にも `: import stack: [...]` を付ける（各要素の ` (test)` は削る） | guff-packages/src/golist.rs:1594-1599（`p.error.err` をそのまま使う。ImportStack を読まない） | needs-port（**実測**：b_test が a を import する形で 2.12.2 は `import cycle not allowed in test (typecheck)`、2.15.0 は `…in test: import stack: [ex/b ex/a ex/b] (typecheck)`、**guff は何も出さない**。テストでない a↔b の循環でも 2.12 / 2.15 とも `import cycle not allowed: import stack: [ex/a ex/b ex/a]`、guff は `seed dep cycle` の警告だけで finding は 0。循環そのものを typecheck として出せていないのが既存の欠け） | M | go/packages/packages_test.go（自作の 2 module：`phase9/cyc`、`phase9/cyc2`） |
| 9 | go/packages golist.go / golist_overlay.go のその他 | behavior | `.go` 以外の file= クエリを ad-hoc package にしない、最も内側の root を選ぶ、overlay の root を `mod.Main` で決める、Windows のエラー文言 | guff-packages（file= クエリも overlay も使わない） | not-applicable | — | overlay_test.go |
| 10 | internal/imports/fix.go ensureEmbedImport | behavior | `//go:embed`（後ろに空白かタブ）があって `"embed"` を import していなければ `import _ "embed"` を足す。未使用として消されそうな `"embed"` は `_` に付け替えて残す | guff-fmt/src/native/goimports/fix.rs:73-118 `get_fixes`（embed の扱いが無い） | needs-port（**実測**：`//go:embed data.txt` / `var s string` に 2.15 は `import _ "embed"` を足す。2.12.2 と guff は足さない） | S | internal/imports/fix_test.go TestEmbedDirective（1682 行〜） |
| 11 | internal/imports/mod.go scanDirForPackage | behavior | module cache の中にある依存 module の root で、subdir を module cache からの相対で計算する | guff-fmt/src/goimports.rs:141（native に module-cache resolver が無く、システムの `goimports` バイナリに任せる） | not-applicable（guff 側にコードが無い。結果はシステムの goimports の版で決まる） | — | mod_test.go TestScanDirForPackageDependencyRoot |
| 12 | go/gcexportdata、internal/gcimporter gcimporter.go | substrate | 先頭が `'i'` なら cmd/export の出力として読む（Go 1.28 の `go list -export` はこの形式を出す） | guff-exportdata/src/reader.rs:73（`'i'` は "not supported" で拒否） | needs-port（Phase 8 substrate #8 の V5 と同じく、Go 1.28 の toolchain で走る日まで要らない。hybrid の dep_source が export data を読むので、その日には全滅する） | L | gcexportdata/example_test.go |
| 13 | internal/gcimporter iexport / iimport の bundle 撤去、gccgoimporter、pkgbits のコメント、deps_test | other | WriteBundle / ReadBundle の廃止、依存制限のコメント | — | not-applicable | — | — |
| 14 | cmd/*、go/callgraph/rta、internal/gocommand、internal/modindex、go.mod の依存 | other | gopls と cmd 専用：deadcode / compilebench / toolstash、RTA の反復順（insertion order）、go コマンドのプロセスグループ kill、x/mod などの更新 | — | not-applicable（golangci の linter から到達しない。RTA を使う linter は無い） | — | — |

---

##### 依存順序と PR 分割

依存関係はほとんど無い。Phase 8 の方針（土台を先に）どおり、命令列を変える SSA を最初に入れ、その上で lint の差が出るかを
golden と OSS tier で確かめる。後は独立していて、どの順でも入れられる。

| 順 | PR | 内容（行） | effort | 検証 |
|---|----|-----------|--------|------|
| 1 | **ssa-v052** | substrate #1 compLit の storebuf 撤去（`comp_lit*` から `is_zero` / `sb` を外し、Store を即時に。`mod.rs:1008` の外側の storebuf も不要になる。assignStmt の storebuf は残す）＋ #2 単一 case の select | M | ssadump v0.52 とブロック単位で比べる（struct / array / slice / map / 入れ子 / `[]*T{{}}` / `map[*T]V{{}: …}` / `x = T{a: x.a}`）。golden 全体と OSS tier で finding の変化を見る（変化 0 でも入れる。上流の SSA と一致させるため） |
| 2 | **modernize-v052** | modernize #1 slicesbackward、#2 stditerators、#3 / #4 stringscut、#6 newexpr（`fd.doc` を取れるようにし、`parse_directive` で判定） | M（各 S×5） | 上流 testdata の slicesbackward / stditerators / stringscut / newexpr の差分を golden と fix の case に入れる。golden を取り直したら fix も取り直す。`phase9/mz` と `phase9/nx` の 2.15 の出力と突き合わせる |
| 3 | **stringintconv-intconv** | passes #1：`intconv.Check` をそのまま移植（runeIsExact / isUntypedConst / sprintfOK / no-fix の枝） | M | fix case は **golangci の合成結果**で作る（`phase9/sic` の 2 形：int16 は合成、int / int64 は conflict でファイルの govet 変更を全部捨てる）。guff の fixer が conflict を同じ単位で捨てるかもここで測る |
| 4 | **goimports-embed** | substrate #10 ensureEmbedImport（AddImport / DeleteImport → SetImportName `_`） | S | TestEmbedDirective の各 case を fmt の golden に入れる |
| 5 | **load-import-cycle** | substrate #8：まず循環を typecheck として出す（既存の欠け。seed が循環で止まらないように）。そのうえで `ImportStack` を付け、` (test)` を削る | M（循環で seed が壊れているなら L） | `phase9/cyc`（test）と `phase9/cyc2`（通常）を 2.15 と比べる |
| 6 | （小）**inline-directive-ws** | passes #4：前段フィルタをタブ区切りにも効くようにする | S | 自作 1 形 |
| — | 持ち越し | modernize #7 embedlit（L）、passes #8 printf go1.27（M、Phase 8 の「PR 15」）、substrate #12 export data `'i'`（L、Go 1.28 待ち） | — | Phase 8 の台帳どおり |

重複：passes #3 / #4 / #5 と modernize #6 は、どれも上流の同じ変更（`astutil.Directives` → `ast.ParseDirective`）から来ている。
guff 側では inline と nilness は既に一致していて、残りは modernize #6（newexpr、既存の欠陥と一緒に直す）と passes #4（タブ）の 2 か所。

### 3.4 golangci-core ＋ staticcheck

#### Phase 9 台帳（core 分）— golangci-lint 2.14.0 → 2.15.0 / honnef.co/go/tools v0.8.1 → v0.8.2

> 読み取り専用で作成（guff は編集していない）。「guff の場所」は `crates/` を省略。
> 測定は scratchpad の `phase9/core-t/` で、`compat/.tools/golangci-lint-{2.14.0,2.15.0}` と
> `target/release/guff`（10/31 00:13 ビルド、HEAD d685ce5d の直前）を同じ入力で走らせた。
> 「測定」と書いた行は 3 者の出力を実際に突き合わせたもの。
> exhaustive / predeclared / iface / revive の**linter 本体**の差分はそれぞれのグループの担当。
> ここに載せるのは golangci-lint 側（settings・wrapper・inventory・`pkg/golinters/*/testdata`）だけで、
> 本体と重なる行には「→ ○○ グループ」と書いた。

##### 凡例

status = `needs-port` / `already-matches` / `not-applicable` / `unsure`。effort = S / M / L。
「既存差」は 2.14.0 の時点で既に guff が違っていたもの（今回の差分を確認していて見つけたもの）。

##### 集計

| グループ | needs-port | already-matches | not-applicable | unsure | 計 |
|----------|-----------:|----------------:|---------------:|-------:|---:|
| golangci-core | 11 | 2 | 5 | 0 | **18** |
| staticcheck | 3 | 3 | 7 | 0 | **13** |
| 計 | 14 | 5 | 12 | 0 | **31** |

needs-port 14 行のうち 5 行（core #5 の警告そのもの、core #10 の field 漏れ、core #11, #14、staticcheck #2）は 2.14.0 からの既存差。

---

##### golangci-core

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | commands/internal/builder.go `createVersion` | behavior | release notes「fix: compute custom version hash with module versions」。custom バイナリの版は `<ver>-custom-gcl-<sha256>` で、2.14 は `path` プラグインのディレクトリ hash だけを入れ、`module`+`version` プラグインは `continue` で飛ばしていた（プラグインの版を上げても版文字列＝cache の salt が変わらない）。2.15 は `module@version` も hash に入れる | guff-lint/src/custom.rs:160-262（生成する main は `guff_lint::cli::main()` を呼ぶだけ）、lib.rs:75 `guff_version()` = `CARGO_PKG_VERSION`、lib.rs:846 `build_salt(guff_version(), …)` | needs-port | M | — |
| 2 | goutil/env.go `Discover`（値レシーバ → ポインタ） | behavior | release notes「fix: env vars loading」。2.14 は `e.vars = goenv.Get(...)` がコピーに代入されて捨てられ、`Get(GOCACHE/GOROOT)` は環境変数しか見ていなかった（`-v` の `[goenv] Read go env` ログは出るのに中身が使われない）。2.15 は `go env GOCACHE` で cgo processor が GOCACHE 配下の issue を落とす。**測定**：cgo パッケージ + `default: all` + `exclusions.generated: disable` で 2.14 は `cgo: 21/21, invalid_issue: 21/6`、2.15 は `cgo: 21/6, invalid_issue: 6/6`。GOCACHE のファイルは `.go` 拡張子を持たないので 2.14 でも invalid_issue が落としており、**最終出力は 2.14 = 2.15**（5 件ずつ同一） | guff-lint/src/exclude.rs:250（`default_go_cache_dir()`）, :509-512、guff-runner/src/cache.rs:168-190（env → プラットフォーム既定 → `go env`）, :251-266 `is_under_go_cache` | already-matches | S | env_test.go（単体テストのみ） |
| 3 | lint/package.go `prepareBuildContext`（#2 の副作用） | behavior | 2.14 は `goroot == ""` の早期 return で `build.Default.GOROOT` も **`build.Default.BuildTags = run.build-tags` も**設定されていなかった。2.15 から両方が効く。pin の依存 module を grep した範囲では analyzer 内で `build.Default.BuildTags` を読むものは無い（gosec の `load` と nakedret の `import.go` は CLI 経路、exhaustruct v5 の `build.Default.GOROOT` は stdlib 判定で `go env` と同値）。depguard の `findGOROOT` は env → runtime → `go env` で結局同じ値 | guff-packages（build tags は go list に直接渡す） | not-applicable | S | — |
| 4 | config/linters_settings.go Gosec.Concurrency | default | `runtime.NumCPU()` → `runtime.GOMAXPROCS(0)`（release notes「gosec: improve concurrency default value」）。並列度だけで finding は変わらない | guff-lint/src/settings.rs:1985（`// DEFERRED: concurrency.`） | not-applicable | S | — |
| 5 | goformatters/gofumpt/gofumpt.go | message | `extra-rules` の deprecated 警告が「`extra.group-params` and `extra.clothe-returns`」に（release notes「gofumpt: improve deprecation logs」）。**guff は 2.14 の文言も含めて警告を一切出していない**（既存差。Phase 8 core #13 は値の読み取りだけ） | guff-lint/src/config.rs:601-605（`extra-rules` を読むだけ） | needs-port | S | — |
| 6 | linters_settings.go ExhaustiveSettings | config-schema | 新キー `explicit-exhaustive`（bool）、`only-types` / `ignore-types` / `ignore-constants`（**正規表現のリスト**）。旧 `ignore-enum-members` / `ignore-enum-types`（文字列）、`explicit-exhaustive-map` / `-switch` は Deprecated として残る。**測定**：`ignore-constants: [West$, Zero$]` を guff は読まず `East, West, Zero` の 3 つを報告（2.15 は `East`、2.14 も新キーを知らないので 3 つ） | guff-lint/src/settings.rs:610-630（旧キー 8 個だけ） | needs-port | S | exhaustive_ignore_enum_members.{go,yml} |
| 7 | golinters/exhaustive/exhaustive.go | behavior | wrapper の書き直し：`exhaustive` と `enumerated` の 2 analyzer に分割（`package-scope-only` は `enumerated` の `p` へ）。旧 3 キーで `the option '…' has been replaced by '…'.` 警告、`check: [map]` は `the value 'map' has been replaced by 'mapliteral'.` 警告のうえ `mapliteral` に読み替え、`e` = 3 つの explicit の OR、旧文字列キーは空文字列なら渡さない（空正規表現が全部を除外するため）、リストは `name-N$` の繰り返しフラグで渡す（#12） | settings.rs:610-630、guff-style/src/exhaustive.rs | needs-port | S | — |
| 8 | golinters/exhaustive/testdata/*.go | message | 文言が `missing cases in switch of type testdata.Direction: testdata.East, testdata.West` → `missing cases in expression switch: East, West`。`exhaustive_cgo.go` の `//go:build ignore`（「the linter doesn't support cgo」）が外れた。**測定**：guff は 2.14 の文言のまま。→ exhaustive グループ（v0.13 → v0.15 本体、`typeswitch` / `only-types` も） | guff-style/src/exhaustive.rs | needs-port | L | exhaustive.go, exhaustive_cgo.go, exhaustive_ignore_enum_members.{go,yml} |
| 9 | linters_settings.go PredeclaredSettings.Mode + predeclared.go | new-option | `mode`（既定 `["shadow"]`）、`declare`→`d` / `shadow`→`s` に変換、未知値は `predeclared: unsupported mode %q` 警告、空なら `s`。`pkglevel: false` を固定で渡す | guff-lint/src/settings.rs:416-421（`ignore` / `qualified-name` だけ） | **done (P9-6)** | S | predeclared_custom.yml |
| 10 | golinters/predeclared/testdata/*.go | message | 文言が `variable real has same name as predeclared identifier` → `real: shadows predeclared identifier`（mode=shadow）/ `copy: same name as predeclared identifier`（declare）。**測定**：guff は 2.14 の文言。**既存差**：`predeclared_custom`（`qualified-name: true`）の struct field `true bool` を 2.14 も 2.15 も報告するが guff は報告しない（method `byte` は報告する）。→ predeclared グループ（v0.2.2 → v0.3.0 本体） | guff-style/src/predeclared.rs:83 | **done (P9-6)** | M | predeclared.go, predeclared_cgo.go, predeclared_custom.{go,yml} |
| 11 | lintersdb/builder_linter.go predeclared `WithLoadForGoAnalysis()` | inventory | predeclared が `fast` から外れる（drift の Inventory 2 行と一致）。**測定**：`linters.default: fast` は 2.14 で 45 linter、2.15 で 44（差は predeclared だけ）。**既存差**：guff の `fast` は `["govet", "errcheck", "ineffassign", "unused"]` の 4 つで、上流の集合（asciicheck, bidichk, … ineffassign, … whitespace）と **ineffassign 以外一つも一致しない**。上流の fast には govet / errcheck / unused は入らない | guff-lint/src/registry.rs:19-20 `FAST_LINTER_NAMES`、config.rs:79 | needs-port | M | — |
| 12 | goanalysis/linter.go `configureAnalyzer` | behavior | 設定キー末尾が `-X$` なら繰り返しフラグとして `-X$` を剥がして Lookup。現状これを使うのは exhaustive wrapper だけ（#7 に吸収） | — | not-applicable | S | — |
| 13 | golinters/revive/revive.go `safeTomlSlice` 削除 | behavior | `arguments` の `map[any]any` → `map[string]any` 変換をやめた（revive v1.17.1 側で吸収）。**測定**：新 testdata の add-constant 引数（map 1 個）で 2.14 = 2.15 | — | not-applicable | S | revive_add_constant.{go,yml} |
| 14 | golinters/revive/testdata/revive_add_constant.{go,yml}（新規） | behavior | add-constant の map 引数（`maxLitCount` / `allowStrs` / `allowInts` / `allowFloats`）。**測定**：上流（2.14 / 2.15 とも）は `"dup"` 3 回目と `42` の 2 件、guff は加えて `1.0` も報告。**既存差**：guff の add-constant は config を一切読まない（rules/add_constant.rs:60-62 のコメント、COMPAT-HARDENING §「add-constant が config を一切読まない」）。→ revive グループ | guff-revive/src/rules/add_constant.rs:13（`STR_LIT_LIMIT = 2` 固定）, :85-97（INT / FLOAT を無条件に報告） | **done (P9-6)** | M | revive_add_constant.{go,yml} |
| 15 | golinters/iface/testdata/fix/{in,out}/iface.go | behavior | iface v1.5.3：`unused` は **exported な interface に SuggestedFix を付けない**（diagnostic は出す）。fix テストの期待が「Pinger / Healthcheck / UserRepository を削除」→「残して 3 件報告、exit 1」に。**測定**（`--fix`）：2.14 は 3 つとも削除、2.15 は 3 つ残して 3 件報告（opaque の `*server` 書き換えは両方とも）、guff は 2.14 と同じく削除。`unusedmethod` の `removalFix` も同じ形に変わっている → iface グループ | guff-style/src/iface.rs:450-454（`remove_fix` を無条件に付ける）, :555 | needs-port | S | fix/in/iface.go, fix/out/iface.go, iface_fix.yml |
| 16 | exitcodes.go / errorutil/errors.go / goutil/env.go `Get` | other | `ExitError` / `PanicError` のメソッドをポインタレシーバに。生成箇所は 2.14 から `&ExitError{}` / `NewPanicError` で `errors.AsType[*ExitError]` も同じなので挙動は変わらない | — | not-applicable | S | — |
| 17 | .golangci.reference.yml / jsonschema（stable） | config-schema | next から stable への文書の繰り上げ：gosec の G407、exhaustruct `allow-empty-blank-assignments`、revive `specify-disable-rule` ディレクティブ、rule `marshal-receiver` / `multiline-if-init` / `use-slices-concat`、identical-switch-branches の `allow-identical-default`。どれも 2.14.0 のコードには既にあった（Go 側の settings 差分に出ない） | guff-style/src/gosec.rs:304, :594、guff-lint/src/settings.rs:602、guff-revive/src/directives.rs:123、guff-revive/src/config.rs:124-126、guff-revive/src/rules/identical_switch_branches.rs:20 | already-matches | S | — |
| 18 | （pin 自体） | other | compat の pin を 2.15.0 へ。`GOLANGCI_LINT_COMPAT`、`compat/.tools`、golden の regen | guff-lint/src/lib.rs:72 | **done (P9-5)** | S | — |

###### メモ（core）

- **#1 の移植方針**：上流の版文字列は Go の module 前提なので byte 一致は無理。等価にすべきは「プラグインの版または
  ソースが変わったら版文字列と cache salt が変わる」こと。`guff custom` が cargo build 時に
  `module@version`（`path` のときはディレクトリ hash）の sha256 を環境変数で渡し、`guff_version()` が
  `option_env!` で `<ver>-custom-gcl-<hash>` を返す形が最小。guff は 2.14 の `path` 側も持っていないので、
  `path` プラグインのソースを書き換えて再ビルドしても issues cache が古い結果を返す（memory「issues cache の salt は
  version 文字列」と同じ穴）。
- **#2 の残り**：guff は `go env -w GOCACHE=…`（GOENV ファイル）を見ない（env → プラットフォーム既定が先）。また
  GOCACHE が解決できないと `is_under_go_cache` が `_cgo_gotypes.go` の basename 判定まで飛ばす（cache.rs:252-254）。
  上流は basename 判定を GOCACHE と無関係に行う。どちらも最終出力は invalid_issue 相当で落ちるので観測できていない。
- **#11 は 2.15 と無関係に直すべき既存差**（`default: fast` の利用者は 4 linter しか走らない）。上流の集合は
  `golangci-lint linters -c <default: fast>` の Enabled 欄をそのまま転記する（44 個、2.15）。
- 測定のついでに見えた CLI の既存差（台帳外）：`guff run` に `--enable-only` と `--max-same-issues` が無い。

---

##### staticcheck（honnef v0.8.1 → v0.8.2）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | analysis/code/code.go `SelectorName` | behavior | `X` が `*ast.IndexExpr` / `*ast.IndexListExpr`（`pkg.S2[int]{Field2: …}` の key を SA1019 が合成した selector）なら `(%s).%s` に `info.TypeOf(x)`。v0.8.1 は `unsupported selector` で panic。**測定**：2.14 は SA1019 が panic して `Running error: can't run linter goanalysis_metalinter`、2.15 は `SA1019: (example.com/m/pkg.S2[int]).Field2 is deprecated: …`、guff は `(?).Field2 is deprecated` | guff-analysis/src/code.rs:1127-1131（`_ => {}` → `selector_name_for` の `(?)`）。呼び出しは guff-staticcheck/src/sa1019.rs:1045-1090 `struct_lit_diagnostics`（Phase 8 staticcheck-checks #2 / staticcheck-ir #11 の続き） | needs-port | S | go1.18/CheckDeprecated/CheckDeprecated_generics.go, go1.18/CheckDeprecated.assist/CheckDeprecatedassist_generics.go |
| 2 | analysis/facts/nilness/nilness.go | substrate | lattice に `AlwaysNilGlobal`（6×6）、`Load` が Inner も設定、非 nil 定数を `{NeverNil, NeverNil}`（pointer-like は MaybeNil）、`== nil` の枝が Outer だけを変える、Merge で「Outer が AlwaysNil 側の Inner を捨てる」。`fn17` が `{NeverNil MaybeNil}` を export するなど fact が変わる。guff には nilness DFA 自体が無い（**既存差**、Phase 8 staticcheck-ir #7）。#7 を移植するときは v0.8.1 ではなく v0.8.2 の形で | 無し（guff-staticcheck/src/sa4023.rs:4「Call/Extract は nilness/typedness に deferred」） | needs-port（staticcheck-ir #7 に合流） | L | analysis/facts/nilness/testdata/src/example.com/Nilness/{Nilness,Nilness_go17,Nilness_go118}.go, Interface/Interface.go |
| 3 | sa4023/testdata/go1.0/issue1737（新規） | behavior | sentinel のグローバル error を返す関数をループ越しに返すと、v0.8.1 は `f2 never returns a nil interface value` + `this comparison is always true` を誤報告（MaybeNilGlobal が Inner に伝わらない）。**測定**：2.14 は 2 件、2.15 は 0 件、guff は 0 件。guff は SA4023 の Call 枝を持たないので**偶然一致**（#2 を入れるときの回帰テストに必須） | guff-staticcheck/src/sa4023.rs:4 | already-matches | S | sa4023/testdata/go1.0/issue1737/issue1737.go |
| 4 | nilness.go `isNil` = `k.IsNil()`（旧 `k.Value == nil`） | behavior | 型パラメータのゼロ値 `var zero T; n <= zero`（`T ~int | ~string`）を nil と取り違えて `unhandled token <=` で panic → SA4023 も `interface conversion` で落ち、2.14 は run 全体が `Running error`。**測定**：Nilness パッケージで 2.14 は Running error、2.15 と guff は同じ 2 件（SA4021 / S1002） | guff-staticcheck/src/sa4023.rs（DFA なし、落ちない） | already-matches | S | Nilness_go118.go `generic4` |
| 5 | go/ir/const.go `nillable` | behavior | `unsafe.Pointer` の nil 定数を `IsNil()` に | guff-ssa/src/const_val.rs:36-39（`is_nil` = `val.is_none()`、`nillable` は DEFERRED なので unsafe.Pointer も既に true）。逆向きの既存差は残る：guff は非 nillable な型パラメータのゼロ値も nil とみなす（#2 / #4 の移植時に `nillable` を入れる） | already-matches | S | const_test.go（単体） |
| 6 | go/ir/builder.go `assignSelectRecvStmt` / `assignLHS` | substrate | case が 1 つの select（`select { case a[f()] = <-ch: }`）は「RHS を評価して受信してから LHS を評価」に。あわせて `emitLocalVar` の source を lhs → 文全体に。x/tools v0.52 の go/ssa にも同じ変更（`tools@v0.52.0/go/ssa/builder.go:1240`）→ xtools グループと共有 | guff-ssa/src/builder/stmt.rs:1060-1080（`self.stmt(comm)` のまま） | needs-port | S | — |
| 7 | go/ir/builder.go 匿名関数の `recvtypeparams` / `recvtypeargs` 共有、func.go `isGenericOrigin` で `info = nil` | substrate | `InstantiateGenerics` モードで instantiated receiver のメソッド内の入れ子 func literal が落ちる不具合と、型情報の早期解放（メモリ）。staticcheck の buildir は `InstantiateGenerics` を使わない（mode を立てるのは go/ir 内と sanity / irutil/visit だけ）ので checks からは観測できない。go/ssa 側は xtools グループ | guff-ssa/src/instantiate.rs:94-207 | not-applicable | S | instantiate_test.go `TestNestedFunctionsInMethodWithInstantiatedReceiver` |
| 8 | go/ir/util.go canonizer | substrate | 受信者付き signature を canonical 化しない（go.dev/issue/81602：generic **メソッド**の method value が同形の関数 instance の signature を汚す）。generic メソッドは Go 1.27 | — | not-applicable（PR 15 / Go 1.27） | S | generic_methods_test.go `TestIssue81602` |
| 9 | go/ir/methods.go / task.go / builder.go `waitForSharedFunction` | substrate | `MethodValue` / `LookupMethod` を無割り当てに（`methodKey`、`existingMethod`）、task.go の `x.transitive` → `u.transitive`（完了印を付けるノードの取り違え）。並列ビルドの性能と待ち合わせだけ | — | not-applicable | S | methods_test.go `TestMethodValueNoAlloc` |
| 10 | go/ir/irutil/stub.go `IsStub` | behavior | `fn == nil` の guard（crash 回避のみ） | guff-analysis passes/facts/purity.rs:223（Phase 8 staticcheck-ir #5） | not-applicable | S | — |
| 11 | internal/xtools-internal（typesinternal classify_call / element.go、astutil/comment.go） | substrate | x/tools v0.51 への再同期：`UsedIdent` を linkname から実装に、`Callee` / `StaticCallee` 追加、`ForEachElement` の引数形、`Directives` を `ast.ParseDirective` に。honnef の checks に呼び出し元が無い（`ClassifyCall(` / `Directives(` を grep して 0 件、`ForEachElement` は go/ir/methods.go の runtime types だけ） | — | not-applicable | S | element_test.go |
| 12 | lintcmd/version、go/loader（`//lint:ignore SA1019` コメント）、go.mod | other | 版 2026.2.2 / 0.8.2、x/tools v0.51 | guff は staticcheck の版文字列を持たない（grep 0 件） | not-applicable | S | — |
| 13 | （drift の「gate would say」の staticcheck-qf / -s / -sa / -st、inline-gofix-sibling） | other | **drift.py の方法論の産物**。5 case とも upstream の 2.14→2.15 は「same」で、guff 対 2.15 の差は各 case の `ratchet.json` と件数が完全に一致する（qf 1/0、s 1/0、sa 0/1、st 8/0、inline-gofix-sibling 2/0）。`compat/drift.py` は ratchet.json を読まない（gate 側の `run.sh:216` だけが `--ratchet` を渡す）ので、受け入れ済みの既存差を毎回「missing / extra」として並べる。上流の鍵集合が 2.14 と 2.15 で同一なので、guff 対 2.15 の差集合は guff 対 2.14 の差集合と同じ（＝ratchet 済み：SA4017 の cross-package purity、SA4006 の IR block 最適化、SA5005 の IR 差、inliner 未移植）。staticcheck 0.8.2 / x/tools 0.52 の変更でも非決定性でもない | compat/drift.py（ratchet を参照しない）、compat/golden/cases/{staticcheck-qf,staticcheck-s,staticcheck-sa,staticcheck-st,inline-gofix-sibling}/ratchet.json | not-applicable | S | — |

###### メモ（staticcheck）

- #13 の改善案（任意）：drift.py の「gate would say」節で、ratchet.json がある case は ratchet との差分だけを出すか、
  `(ratchet n/m)` を併記する。そうすれば次の bump でもこの 5 行の説明が要らなくなる。
- 0.8.2 の check 本体（`staticcheck/*/sa*.go`）は testdata 以外に差分が無い。挙動差は SelectorName（#1）と
  nilness（#2-#4）だけ。

---

##### 依存順序と PR 分割

依存は 2 本だけ：core #6 → #7（settings の構造体が先）、staticcheck #2 は Phase 8 staticcheck-ir #7（nilness DFA 本体）と同じ PR。
それ以外は独立。

| PR | 中身 | 依存 | effort | 検証 |
|----|------|------|--------|------|
| A | **pin 移動の前提**：core #18（`GOLANGCI_LINT_COMPAT`、`compat/.tools`、golden regen）。regen 後に drift の gate 節が出す行のうち #13 の 5 case は ratchet 済みと確認 | — | S | golden 全 case、drift の再測 |
| B | **staticcheck SelectorName**：staticcheck #1（`IndexExpr` / `IndexListExpr` → `(TypeOf(x)).Sel`） | — | S | golden に CheckDeprecated_generics.go + assist を追加（2.14 では panic するので 2.15 で regen） |
| C | **exhaustive の settings / wrapper**：core #6, #7（新キー、旧キーの警告と合流、`map`→`mapliteral`、空文字列を渡さない、`package-scope-only` の行き先） | 本体（core #8）と同じ PR か直前。本体は exhaustive グループ | S（本体込みで L） | exhaustive_ignore_enum_members.{go,yml}、警告文の stderr |
| D | **predeclared の settings / inventory**：core #9（`mode`）、core #11 のうち predeclared を fast から外す部分。本体の文言（core #10）は predeclared グループと同じ PR | 本体と同時 | S | predeclared_custom.yml、`linters -c default:fast` |
| E | **`fast` 集合の既存差**：core #11（`FAST_LINTER_NAMES` を上流の 44 個に） | — | M | `linters.default: fast` で上流の Enabled 欄と集合一致 |
| F | **iface unused の fix**：core #15（exported なら SuggestedFix なし、unusedmethod も同様） | — | S | golangci の fix/in → fix/out（fix tier） |
| G | **警告の小物**：core #5（gofumpt `extra-rules` の deprecated 警告） | — | S | stderr 1 行 |
| H | **custom の版 hash**：core #1（`module@version` / path の dir hash → 版文字列と cache salt） | — | M | `guff custom` を 2 つの版で建てて `version` と cache が分かれること |
| I | **go/ir・go/ssa の single-case select**：staticcheck #6（xtools グループの go/ssa v0.52 と同じ PR に） | — | S | 1 case の select で LHS に呼び出しを置いた fixture |
| J | **nilness DFA**（Phase 8 staticcheck-ir #7 に合流）：staticcheck #2 を v0.8.2 の形で、#3 / #4 / #5 の回帰 fixture 込み | staticcheck-ir #7 | L | Nilness/*.go の fact、issue1737（0 件のまま）、generic4（落ちない） |
| — | core #14（add-constant が config を読まない）は revive グループの PR に | — | M | revive_add_constant.{go,yml} |

effort 合計（このグループ分、他グループ本体を除く）：S × 7（A, B, C, D, F, G, I）、M × 2（E, H）、L × 1（J、既存の ir #7）。

### 3.5 linters-a

#### Phase 9 台帳 — linters-a（exhaustive / predeclared / gocognit / iface）

> 2.14.0 → 2.15.0。読み取り専用で作成（guff は編集・ビルドしていない）。
> 上流の挙動は scratch（`scratchpad/phase9/la/`）で **golangci-lint 2.15.0 バイナリ**（`scratchpad/phase9/bin/golangci-lint`）と
> guff の既存 release バイナリ（`target/release/guff`、10/31 ビルド＝2.14.0 相当）を同じ fixture に当てて確かめた。
> 「guff の場所」は `crates/` を省略。drift の行は `drift-20261102T004729Z/REPORT.md`。

##### 集計

| グループ | needs-port | already-matches | not-applicable | unsure | 計 |
|---|---:|---:|---:|---:|---:|
| exhaustive | 21 | 4 | 1 | 0 | **26** |
| predeclared | 6 | 2 | 1 | 0 | **9** |
| gocognit | 6 | 1 | 2 | 0 | **9** |
| iface | 4 | 1 | 1 | 0 | **6** |
| **合計** | 37 | 8 | 5 | 0 | **50** |

---

###### exhaustive（nishanths/exhaustive v0.13.0 → v0.15.0：`passes/exhaustive` + `passes/enumerated` + `internal/passes/finder` への全面書き直し）

v0.15.0 の testdata は module zip に**入っていない**（`passes/*/testdata/test` が別 module なので zip から落ちる）。GitHub の `v0.15.0` タグから取った（`scratchpad/phase9/la/src/exhaustive-v0.15.0/`）。

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | golinters/exhaustive/exhaustive.go, linters_settings.go | new-option | 新キー `explicit-exhaustive` / `only-types` / `ignore-types` / `ignore-constants`（リスト）。golangci は `typeonly-0$` 形の繰り返し flag で渡す（goanalysis `configureAnalyzer` の `-X$` 剥がし、core 側の変更） | guff-lint/src/settings.rs:610 `ExhaustiveSettings` に無い。serde は未知キーを黙って捨てるので**設定しても無効** | needs-port | M | golangci exhaustive_ignore_enum_members.{go,yml}（`ignore-constants: [West$, Zero$]`） |
| 2 | golinters/exhaustive/exhaustive.go | new-option | 旧キー 4 つ（ignore-enum-members / ignore-enum-types / explicit-exhaustive-{switch,map}）は deprecated warning 付きで存続。`check: [map]` は warning を出して `mapliteral` に読み替え。`mapliteral` / `typeswitch` が値として有効。未知の値は `invalid value for flag -check` で analyzer error | settings.rs:3784 `to_guff_exhaustive`：`"switch"` / `"map"` の文字列一致だけ。`mapliteral` を書くと map 検査が**黙って無効**、未知値も黙殺 | needs-port | S | — |
| 3 | golinters/exhaustive/exhaustive.go `"e"` | behavior | `e = explicit-exhaustive \|\| -switch \|\| -map`。`explicit-exhaustive-map: true` だけでも switch（と type switch）が enforce 限定になる（scratch で確認：`switch a` が消える） | guff-style/src/options.rs:519-522 が switch / map 別々 | needs-port | S | golden `exhaustive-explicit` |
| 4 | passes/exhaustive/check.go checkSwitch / formatMissingNames / nameString | message | `missing cases in expression switch: A, B`。同一パッケージは裸の名前、他パッケージは `pkgname.X`。同値の定数は**全部** `\|` で連結（`X1\|Y1`）。並びは各群の先頭定数の位置順 | exhaustive.rs:773 `switch_finding`（`pkg.T` / `pkg.X` で修飾）、:804 `report_switch`、:708 `missing_members`（同値群から先頭 1 つだけ） | needs-port | S | check/swtch/general、golden `exhaustive*` 全行、scratch `X1\|Y1` |
| 5 | check.go checkMapLiteral | message | `missing keys in map literal: …`（名前規則は #4 と同じ） | exhaustive.rs:835 `map_finding`（`missing keys in map of key type …`） | needs-port | S | check/mapliteral/** |
| 6 | check.go checkSwitch reportMissingDefault | message / behavior | `missing default case`（型名なし）。**missing cases も同じ位置に続けて出す**（v0.13 は default を出したら return）。uniq-by-line 既定 true なら見え方は 1 件 | exhaustive.rs:804 `report_switch`（:816 で return） | needs-port | S | drift `exhaustive-default-case-required` bad.go:16/41、directives.go:21/43/58/139/321、foreign.go:10/20/62/82 |
| 7 | exhaustive.go parseDirectives（`ast.ParseDirective`） | behavior | 判定が `ast.ParseDirective`（go1.26）に。エラー文言 `error parsing comment directives: invalid name "x"` / `conflicting directives` / `args not allowed`。`//exhaustive:`（名前空）は**directive ではない**ので無視。`//exhaustive:ignore // 理由` は args 扱いで**エラー**（scratch 確認、実リポジトリで踏みやすい） | exhaustive.rs:927 `parse_directives`（空白で切って後ろは捨てる、`invalid directive ""`）、:973 `invalid_directive` | needs-port | M | check/swtch/directive、drift directives.go:104/116/127/184/311/321 |
| 8 | check.go checkSwitch / checkMapLiteral | behavior | directive エラーを報告したら**そこで return**（検査しない）。v0.13 は読めた分の directive で検査を続け missing も出していた | exhaustive.rs:1036 `run`（エラー後も finding を積む） | needs-port | S | drift directives.go:116/127/184（missing が消える）、explicit 104 |
| 9 | exhaustive.go parseDirectives | new-option | 新 directive `defrequire=0` / `defrequire=1`（旧名 ignore-/enforce-default-case-required は別名）で switch ごとに default 必須を上書き。map では ignore / enforce として読まれなくなった | exhaustive.rs:911 `Directives` / :927 | needs-port | S | check/swtch/defrequire{,/directive1}、drift explicit 172/178 が消える |
| 10 | check.go `skip(reason)` | new-check | enforce 付きで検査できないとき報告：`enforce directive present and expression switch not checked: no switch expression` / `…: switch expression type is not an enumerated type`、map `…map literal not checked: key type is not an enumerated type`、type switch `…: expression in type assertion is not basic interface` | 無し | needs-port | S | check/swtch/directive 21/25/78（scratch で guff 側 0 件） |
| 11 | check.go checkSwitch / checkMapLiteral | behavior | case / key は `TypesInfo.Types[expr].Value` があれば何でも数える（`case 0:`、`T1(3)`、`X+1`、untyped literal） | exhaustive.rs:638 `expr_const_val` / :618 `strip_conversions`（Ident / Selector の定数だけ） | needs-port | S | check/swtch/general f9（guff だけ general/file.go:203 / :227 に出る） |
| 12 | check.go checkMapLiteral | behavior | 型は `Types[compLit]`（elided な内側 `{…}` も検査、`[]*map[K]V{{…}}` は `unpointer`）、**空リテラルも検査**。位置は `compLit.Pos()`（elided なら `{`） | exhaustive.rs:850（`lit.ty` 無しで return）/ :855（空で return）/ :1022 | needs-port | S | general f14 / 68 / 122-139 / 344-349、drift directives.go:270/274/277/301-303 |
| 13 | check.go compositeLitComments | behavior | map の directive は最寄りの AssignStmt（cmap）または ValueSpec（Doc / Comment / cmap）＋親 GenDecl（Doc / cmap）で**止まる**。Return / Call / FuncLit 越しは読まない | exhaustive.rs:1005 `map_related_comments` / :981 `map_related_node`（スタック全体） | needs-port | S | check/mapliteral/commentassoc、drift explicit 191（return）/ 208（call 引数）が消える |
| 14 | check.go checkMapLiteral | behavior | directive の解析は「map 型のリテラル」なら key が enum でなくても行う（`map[any]any{}` に出る） | exhaustive.rs:1021 `is_checked_map_literal`（型式と要素が要る） | needs-port | S | check/swtch/directive g1（57/61） |
| 15 | check.go hasTypeName / proceedInclExclPatterns | behavior | type パターンは**タグの型名**で照合（alias なら alias 自身の名前、TypeParam も）。pkg-level のみ。`only-types` が当たれば ignore より優先。enforce はパターンを無視 | exhaustive.rs:694 `type_ignored`（unalias 後の enum 名で照合） | needs-port | S | check/swtch/pattern{,/includetype}（scratch：T を ignore しても `switch a`（Al）は出る） |
| 16 | check.go needValues | behavior | const パターンは pkg-level 定数だけ（関数内定数は当たらない） | exhaustive.rs:687 `member_ignored`（常に照合） | needs-port | S | check/swtch/pattern（`Y0` は no effect） |
| 17 | passes/enumerated（旧 enum.go hasIgnoreDecl 削除） | behavior | **宣言側 `//exhaustive:ignore` と宣言 doc の不正 directive 診断が廃止**（Phase 8 exhaustive-sumtype #3 / #4 を戻す） | exhaustive.rs:185 `has_ignore_decl`、`DeclDirectives`、:276 `dep_ignores`、:340 `find_enums` | needs-port | M（削除） | drift `exhaustive-ignore-decl` 全行（decl/declignore.go:41/53/65/83/86/116/129 消滅、:145/149、enum_switches.go:23/25 出現、メンバ増） |
| 18 | enumerated.go permittedEnumeratedType / Constant | behavior | 定数の型が**そのまま** `*types.Named` のときだけ enum メンバ（alias 型で宣言した定数は外れる＝Phase 8 #6 を戻す）。underlying は `IsNumeric\|IsString`（complex も enum に）。型パラメータ付き Named は除外 | exhaustive.rs:143 `possible_enum_member`（`named_type_name` が unalias）、:120 `valid_basic_underlying`（complex 不可） | needs-port | S | enumerated testdata enum/typ、scratch（`const Z Al = 7` は外れ、`type C complex128` が出る） |
| 19 | check.go knownEnumeratedType | behavior | Alias は Rhs を辿り Named で引く。TypeParam は対象外（v0.13 の型パラメータ対応は消滅） | exhaustive.rs:586 `unalias_readonly` 後に Named 以外は None — 結果は同じ | already-matches | S | — |
| 20 | enumerated `-p`（golangci は package-scope-only を enumerated 側に渡す） | behavior | requirePkgLevel | exhaustive.rs:426（型の parent が package scope。定数は型と同じ block なので isPkgLevel と同値） | already-matches | S | enumerated testdata enum/pkglevel |
| 21 | enumerated.go run（AllObjectFacts） | substrate | 依存パッケージの enum は fact で引く | exhaustive.rs:475 `import_enum_members` / :499 `enum_members_from_scope`（他パッケージは exported 定数しか要らないので scope で足りる） | already-matches | S | enumerated testdata depchain |
| 22 | check.go checkTypeSwitch | new-check | `check: [typeswitch]` で type switch の網羅検査。`missing cases in type switch: *types.Basic, *types.Pointer`。empty interface / `error` は対象外、default で網羅（-d）、`A or B`（alias 併記）、並びは cmpfn（非 alias→pkg nil→自 pkg→pkg path→名前→非ポインタ） | 無し | needs-port | L | check/typeswitch/**（gostd は golangci で 3 件。general / alias / def / pattern / casenil は body 無し func で **golangci の typecheck が落ちる**ので fixture に body を足す） |
| 23 | internal/passes/finder | substrate | type switch の候補は**推移的依存すべて**（stdlib 含む）の top-level 型宣言（fact）。`types.Implements(T)` / `(*T)` | guff の依存は export data。推移的依存の package scope から TypeName を列挙し、Implements を当てる経路が要る | needs-port | L | typeswitch/gostd、dpkg |
| 24 | exhaustive.go `-casenil`、enumerated `-i` / `-B` | other | golangci は配線しない | — | not-applicable | — | — |
| 25 | golinters/exhaustive `"g": true` | behavior | 生成ファイルも検査（pin、変更なし） | settings.rs:628 | already-matches | S | — |
| 26 | golangci testdata exhaustive_cgo.go | other | `//go:build ignore` が外れ cgo ケースが有効に | golden に `exhaustive-gcl-cgo` が無い（`iface-gcl-cgo` と同型で足す） | needs-port | S（fixture） | exhaustive_cgo.go |

###### predeclared（nishanths/predeclared v0.2.2 → v0.3.0：AST 走査から `TypesInfo.Defs` / `Implicits` ＋スコープ解決へ全面書き直し）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | passes/predeclared/predeclared.go check / handleObject | behavior | `Defs`（nil 以外）と `Implicits[ImportSpec\|CaseClause\|Field]` の全オブジェクトを見る。shadow は `lookupWhile(obj.Parent(), name, obj.Pos())` が Universe に当たるか。位置ごと・mode ごとに 1 回 | guff-style/src/predeclared.rs:103 `run`（宣言の種類ごとの AST 列挙） | **done (P9-6)** | M | testdata/example.go、src/**、golangci predeclared{,_custom,_cgo}.go |
| 2 | predeclared.go reportAtMostOnce | message | `len: shadows predeclared identifier` / `len: same name as predeclared identifier`（種類名が消えた） | predeclared.rs:79 `maybe_report` | **done (P9-6)** | S | drift `predeclared` bad.go:7/9/11/13/16/19/27×2 |
| 3 | golinters/predeclared `mode` | new-option / default-change | `mode: [shadow, declare]`。既定 `shadow`（`s`）。未知値は warning、空なら `s`。両方なら d → s の順で同じ位置に 2 件（uniq-by-line で 1 件に見える） | settings.rs:416 `PredeclaredSettings` に無い | **done (P9-6)** | S | predeclared_custom.yml |
| 4 | predeclared.go handleObject（shadow） | default-change | shadow はフィールド・メソッド・ラベルを報告しない。`qualified-name: true` は declare モードでしか効かない → **既定ではメソッド名が出なくなる**。package 句（Defs が nil）も出ない | predeclared.rs:139（`package name`）、:146（qualified でメソッド） | **done (P9-6)** | S | drift bad.go:29 `method delete` 消滅、golangci predeclared.go `func (t *t1) byte()` |
| 5 | predeclared.go（Defs ベース） | behavior | 拾う範囲が増える：range の `:=`（`for append := range 3`）、type switch の symbolic 変数（Implicits[CaseClause]）、名前の無い import で package 名が predeclared、型パラメータ、declare+q の struct field / interface method | predeclared.rs:165 `DEFERRED (qualified=true)`、RangeStmt を見ていない | **done (P9-6)** | S | drift bad.go:22 `append` 出現、src/field、src/syncmap |
| 6 | golinters/predeclared `ignore` | behavior | カンマ連結して split ＋ TrimSpace（2.14 から同じ） | predeclared.rs:111 リスト要素をそのまま集合に（`" len"` が効かない） | **done (P9-6)** | S | src/ign |
| 7 | substrate（Scope.LookupParent / Implicits） | substrate | pos 付き LookupParent、Implicits の記録 | guff-types/src/scope.rs:309 `lookup_parent`（scope_pos を見る）、stmt.rs:212 `record_implicit`（resolver.rs:207/263 import、stmt.rs:1540 case clause、signature_check.rs:186/237 field） | already-matches | S | — |
| 8 | golinters/predeclared LoadModeTypesInfo | other | `fast` グループから外れた（drift の Inventory） | guff-lint/src/registry.rs:20 `FAST_LINTER_NAMES` に元々無い。※ この表は 4 linter だけで golangci の fast 全体とは前から合っていない（別件） | already-matches | S | — |
| 9 | golinters/predeclared `"pkglevel": false` | other | pin | guff にオプションが無い | not-applicable | — | — |

###### gocognit（uudashr/gocognit v1.2.1 → v1.3.0）

golangci は `gocognit.Analyzer` を**使わず** `ComplexityStats(f, fset, stats)` をファイルごとに呼ぶ（LoadModeSyntax）。なので再帰は「型なし・1 ファイル内」の判定になり、上流 testdata `f/` の期待値（型あり Analyzer）とは**一致しない**。golden は golangci の出力を正にする。

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | gocognit.go scanComplexity / recursion.go recursiveNodes | behavior | 再帰は**呼び出しごとではなく関数ごとに +1**（SCC か自己ループ）。Fibonacci 3 → 2 | gocognit.rs:344 `visit_call`（名前一致の呼び出しごとに +1） | **done (P9-6)** | M | a/a.go・b/b.go Fibonacci、f/f.go Fib |
| 2 | recursion.go RecursiveFuncs(files, nil) / resolveCall | behavior | 型なし解決：Ident は parser の `Obj`（body のある FuncDecl）、Obj 無しは同ファイルの非メソッド関数名。Paren / `F[T]` / `F[T,U]` を剥がす。メソッドは自分の receiver 経由の自己呼び出し（`n.Loop()`）だけ。**1 ファイル内の相互再帰**も拾う。closure 内の呼び出しも数える | gocognit.rs:344（`G[T](x)`、`t.M()`、相互再帰 A↔B を数えない）。parser の Obj 解決は guff-ast/src/parser_resolver.rs:42（parser.rs:3240 で既定実行）にある | **done (P9-6)** | M | scratch `g/g.go`：golangci は A=1,B=1,`(T).M`=1,G=1,Lit=1、`f/`：MutualA=1（別ファイルの MutualB は見えない）、Walk=1、Step 無し |
| 3 | recursion.go resolveCall（Obj） | behavior | 局所変数で関数名を shadow した `Self(n)` は再帰ではない（Obj が var を指す） | gocognit.rs:345 名前だけで判定（guff だけ `Self` を 1 と出す。2.14 時点からの差） | **done (P9-6)** | S | scratch g/g.go:11 |
| 4 | gocognit.go visitIfStmt | behavior | `else { … }` の中は nesting +1 | gocognit.rs:250（else ブロックで nesting を上げない） | **done (P9-6)** | S | a/a.go・b/b.go IfElseNested（3 → 4） |
| 5 | gocognit.go collectBinaryOps | behavior | 論理演算列で `ParenExpr` を透過（`a && (b && c)` は 1 列） | gocognit.rs:365 `collect_binary_ops_expr`（括弧で止まる） | **done (P9-6)** | S | e/e.go 全 9 関数（guff は Parens / ParenThenOp / ParenInsideIf / Nested で +1）、a ComplexLogicalSeq3/5 |
| 6 | gocognit.go ComplexityStatsForFiles parseDirective | behavior | `//gocognit:ignore`（diff 外・2.12 から） | gocognit.rs:7 `DEFERRED` | **done (P9-6)** | S | scratch h/h.go（guff だけ `Ign` を出す） |
| 7 | gocognit.go 報告位置 | position | `fn.Pos()`（変更なし） | gocognit.rs:424 `f.ty.pos()` | already-matches | — | — |
| 8 | cmd/gocognit `-exact-recursion`、PossibleMethodRecursion | other | CLI のみ | — | not-applicable | — | — |
| 9 | gocognit.go run（Analyzer、TypesInfo で RecursiveFuncs） | other | golangci は Analyzer を使わない | — | not-applicable | — | f/（型ありの期待値） |

###### iface（uudashr/iface v1.5.1 → v1.5.3）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| 1 | unused/unused.go removalFix | behavior | **exported な interface には SuggestedFix を付けない**（診断は出す）。結果 golangci の fix testdata は `expected_exitcode 1` に | guff-style/src/iface.rs:454（常に fix） | **done (P9-5)** | S | unused/testdata/src/basic（Executor / Runner / Writer は残り sender / scheduler だけ消える。scratch --fix で guff は全部消す）、golangci fix/{in,out}/iface.go |
| 2 | unusedmethod/unusedmethod.go removalFix | behavior | exported なメソッド（`field.Names[0].IsExported()`）は fix 無し（unexported interface 上でも） | iface.rs:555（常に fix） | **done (P9-5)** | S | unusedmethod/testdata/src/basic（`Debug` / `Set` が残る。scratch で guff は消す） |
| 3 | unusedmethod.go `fn.Origin()` | behavior | 使用判定を Origin で：インスタンス化した generic interface（`Optional[int]`、`Iter[T]`、method expr `Callback[T].Call`、method value）経由の使用を宣言側の使用とみなす。**Phase 8 iface-canonicalheader #6 の「使用扱いにしない」を戻す** | iface.rs:516 insert / :533 `remove(&sel.obj())`（インスタンスのメソッド ID のまま） | **done (P9-5)** | S | unusedmethod/testdata/src/typeparams（guff だけ 5 件余分）、drift `iface-edges` methods.go:24 `getter.Get` 消滅 |
| 4 | substrate `Func.Origin` | substrate | インスタンス化で複製したメソッドの origin | guff-types/src/object/func.rs:127 `func_origin`、subst.rs:513（`copy.origin = f.Origin()` を平坦化） | already-matches | S | — |
| 5 | golangci testdata iface/fix | other | fix/in・out に exported の want が増えた | golden `iface-gcl-fix` / fix tier の取り直し（#1 と同じ PR） | **done (P9-5)** | S（fixture） | fix/in/iface.go、fix/out/iface.go |
| 6 | unused/doc.go、unusedmethod/doc.go、Analyzer.Doc | other | 文書のみ | — | not-applicable | — | — |

---

##### drift 行の対応（REPORT.md の全行）

| case | 行 | 説明する行 |
|---|---|---|
| exhaustive / -explicit / -default-case-required | 全 message の書き換え | exhaustive #4 / #5 / #6 / #7 |
| exhaustive | directives.go:116 / 127 / 184 の missing 消滅 | #8 |
| exhaustive | directives.go:321（`//exhaustive:` が error でなくなり missing が残る） | #7 |
| exhaustive | directives.go:270 / 274 / 277 / 301-303 出現 | #12（elided・空リテラル） |
| exhaustive | directives.go:58 に missing cases が出る（default 付き） | #6 |
| exhaustive-default-case-required | 全 switch で「missing default case」＋「missing cases」の 2 行 | #6 |
| exhaustive-explicit | 104 の missing 消滅 | #8 |
| exhaustive-explicit | 191（return）/ 208（call 引数）消滅 | #13 |
| exhaustive-explicit | 172 / 178（default-case 系 directive の map）消滅 | #9 |
| exhaustive-ignore-decl | decl doc の不正 directive 7 行消滅、ignore されていたメンバ・型が出る（:145 / :149、enum_switches.go:23 / :25、メンバ増） | #17 |
| exhaustive-ignore-decl | 名前の修飾が消える | #4 |
| iface-edges | methods.go:24 `getter.Get` 消滅 | iface #3 |
| predeclared | message 書き換え | predeclared #2 |
| predeclared | bad.go:22 `append` 出現 | predeclared #5 |
| predeclared | bad.go:29 `method delete` 消滅 | predeclared #4 |
| Inventory | predeclared が fast から外れる | predeclared #8 |

---

##### PR 分割

| PR | 内容 | 行 | effort | 検算 |
|---|---|---|---|---|
| A | **iface 2.15**：exported の fix 抑止（unused / unusedmethod）、unusedmethod の Origin 照合 | iface #1-#3, #5 | S | golden `iface-v151-*` を v1.5.3 testdata に差し替え（typeparams を足す）、`iface-gcl-fix` と fix tier を取り直し、`iface-edges` methods.go:24 |
| B | **gocognit v1.3.0**：再帰（1 ファイル内 SCC・型なし解決）、else nesting、括弧透過、`//gocognit:ignore`、shadow | gocognit #1-#6 | M | golden `gocognit` に upstream testdata a / b / e / f を golangci 2.15 の出力で（f は golangci の値＝MutualA 1 など）、scratch g/h.go 相当（相互再帰・shadow・generic・closure・ignore）を足す |
| C | **predeclared v0.3.0**：Defs / Implicits ＋ LookupParent で書き直し、`mode`、文言、ignore の TrimSpace | predeclared #1-#6 | M | golden `predeclared` 取り直し（bad.go:22/29）、上流 testdata（example.go・src/**）と golangci の predeclared{,_custom,_cgo} を case 化（custom は mode: [shadow, declare] + uniq-by-line 既定） |
| D | **exhaustive v0.15.0 本体（switch / map）**：文言、directive（ParseDirective・return・defrequire・skip 診断）、定数式、map（Types[compLit]・空・comment 関連）、パターン（タグの型名・pkg-level・only-types）、enumerated（宣言側 ignore 削除・Named 厳密・complex）、設定（新キー・deprecated・`e` 統合・check 値） | exhaustive #1-#18, #26 | L | golden `exhaustive` / `-explicit` / `-default-case-required` / `-ignore-decl` 取り直し、v0.15.0 tag の `check/swtch/**` と `check/mapliteral/**`、golangci exhaustive{,_cgo,_ignore_enum_members}。`exhaustive-ignore-decl` は「宣言側 ignore が効かない」ことの固定に役割を変える |
| E | **exhaustive typeswitch**：finder（推移的依存の型宣言）＋ Implements、文言と並び | exhaustive #22, #23 | L | v0.15.0 `check/typeswitch/**`（body を足して golangci で通る形に）、gostd（`go/types`・`net`） |

依存：D の後に E（directive / パターン / skip を共有）。A・B・C は独立。

##### 付記（範囲外で見つけたもの）

- **guff が package ごと黙って何も出さない**：`func g[P T](p P) { switch p {} }`（T は `type T int`、制約に非 interface 型）を含むパッケージで、exhaustive・predeclared・govet が全部 0 件、終了コード 0（golangci 2.15 は正常に解析）。`switch any(p)` にすると戻る。型検査が偽のエラーを出して、それが表示されずに捨てられているように見える。再現：`scratchpad/phase9/la/exh3/`。types 担当に回すこと。
- predeclared #8 の `FAST_LINTER_NAMES`（`linters.default: fast`）は 4 linter しか無く、golangci の fast（LoadModeSyntax の linter 全部）と前から合っていない。

### 3.6 linters-b

#### Phase 9 台帳 §4（linters-b）: golangci-lint 2.14.0 → 2.15.0

対象: errcheck v1.20.0→v1.30.0、revive v1.17.0→v1.17.1、loggercheck v0.12.0→v0.13.0、wrapcheck v2.12.0→v2.13.0、
paralleltest v1.0.15→v1.0.16、rowserrcheck（golangci fork c5f79b8→e0740d5）、canonicalheader（golangci fork a25c71c→e18e5c5）、
golines v0.15.0→v0.16.0（formatter）、bidichk v0.3.3→v0.3.4。

凡例は PHASE8-LEDGER §4 と同じ（status = `needs-port` / `already-matches` / `not-applicable` / `unsure`、effort = S / M / L）。
「実測」は scratchpad で golangci-lint v2.15.0（`compat/.tools/golangci-lint-2.15.0`、go1.27.0 ビルド）と
guff release バイナリ（d685ce5 時点、`target/release/guff` を scratchpad に複製）を同じ fixture に当てた結果。
fixture は `scratchpad/phase9/exp/{misc,revue,rv2,gold}`。

golangci 側の wrapper（`pkg/golinters/<name>/`、`pkg/goformatters/golines/`）は 9 linter とも **2.14.0 と同一**。
例外は revive.go の `safeTomlSlice` 削除のみ。settings キーの新設・既定値変更はこの 9 つには無い。
ただし golangci 本体の `pkg/goutil/env.go` の修正（R1）が revive の挙動を大きく動かす。

##### drift（compat/drift.py --candidate 2.15.0）との対応

| case | drift の行 | 説明する行 |
|------|-----------|-----------|
| canonicalheader / canonicalheader-exclusions | methodonly.go:17–21 の +5 ×2 | C1 |
| loggercheck | stringer/v0120_stringer.go:28:24, 33:24 の −2 | L1 |
| revive | context-keys-type `untyped string`→`string`、+time-naming（bad.go:50）、+epoch-naming（extended_bad.go:438） | R1（golangci の GOROOT 修正）。guff は**既に 2.15 側を出している**（R2） |
| revive | +unhandled-error 23 件（extended_bad.go 97〜433、fmt.Print* / errors.New） | R1 |
| revive | +unexported-return（unexported_return_alias.go:45:31 `StdAlias`） | R1 |
| revive | +function-length（extended_bad.go:294:1、max 50 but got 61） | R7 |
| revive | +add-constant '1'（extended_bad.go:450:12, 641:8 = `go func(){ x = 1 }()`） | R4 |
| errcheck | 無し | — （新しい形を踏む fixture が golden に無いだけ。E1–E5 は実測で差が出る） |

`revive` case を guff と 2.15.0 で突き合わせると gcl-only 27 行、guff-only 0 行（`exp/gold/revive.{gcl,guff}`）。
27 行の内訳は unhandled-error 23 + unexported-return 1 + function-length 1 + add-constant 2 で、すべて下の行で説明できる。

###### errcheck（kisielk/errcheck v1.20.0 → v1.30.0）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| E1 | errcheck/errcheck.go `checkRangeStmt` | new-check | range-over-func（`func(yield func(K, V) bool)`、yield の引数 1〜2 個・戻り bool）で error 側の変数を受けないと報告。受け手が無ければ **既定で** `for` の位置（`stmt.Pos()`）に、`_` なら `-blank` 時だけ `_` の位置に。call を渡すので文言は `s.Seq` のように名前付きになり、`ignoreCall` で exclude も効く。最初の 1 件で break | guff-errcheck/src/lib.rs:136-143（WANTED に RangeStmt が無い） | **done (P9-6)** | M | testdata/iter.go, errcheck/testdata/src/a/main.go, src/blank/main.go（実測: `exp/misc/ec` の 25,28,30,35,39,42 行と blank 時 32,37 行が gcl-only） |
| E2 | errcheck.go Visit `*ast.ExprStmt` | new-check | 文としての `<-ch`（括弧は剥がす、`(<-ch)` も）で要素型が error を実装すれば `unary.OpPos` に短い文言で報告。**既定で有効**。select の `case <-errCh:` も ExprStmt なので同じ | lib.rs:269-278 `visit_expr_stmt` | **done (P9-6)** | S | testdata/chan.go, src/a/main.go（実測 50:2, 51:3, 52:2, 61:7） |
| E3 | errcheck.go `checkAssignment` unary 枝 | new-check | `-blank` 時: `_ = <-ch` / `_, ok = <-ch` / `var _ = <-ch` は lhs[0] が `_` なら報告（単一 RHS は括弧を剥がす）。多値 `_, x := <-ch, 1` も。`v, _ := <-ch` は報告しない | lib.rs:357-424 `check_assignment` | **done (P9-6)** | S | testdata/chan.go, src/blank/main.go（実測 54,55,57,58,62 行） |
| E4 | errcheck.go `errorsByArg` `Types[call.Fun].IsType()` | behavior | 型変換 `(*T)(nil)` は呼び出しでない。`-blank` で `var _ error = (*convError)(nil)` を報告しなくなる。guff は **call** の operand mode を見ているので変換（Value モード）を通してしまう | lib.rs:545-562 `errors_by_arg`（`call.fun` の TypeExpr を見る必要） | **done (P9-6)** | S | src/blank/main.go 末尾（実測: guff-only 78:5） |
| E5 | errcheck.go `isErrorsAsType` + excludes.go `errors.AsType` | behavior | `errors.AsType[...]` を `ignoreCall` で**無条件に**無視（`disable-default-exclusions` でも効く）。型情報で errors 以外と分かっても最後は `pkgIdent.Name == "errors"` で真になる（綴りが `errors` なら他パッケージでも無視）。既定除外リストにも追加 | lib.rs:425-427 `ignore_call`, excludes.rs:5-45 | **done (P9-6)** | S | testdata/errors_astype.go（go1.26）, errcheck_go126_test.go（実測: guff-only 69:24, 70:2） |
| E6 | errcheck.go `isErrorType` maybeUnalias / nil ガード | behavior | `types.Implements` は内部で Unalias するので観測差なし | lib.rs:600-632 | not-applicable | S | — |
| E7 | errcheck.go `i < len(isError)` / `i < len(rhs)` | behavior | 範囲外 panic のガード | lib.rs:375 `is_error.get(i)`, :399 `rhs.get(i)` | already-matches | S | — |
| E8 | errcheck.go `filterEnv` / `LoadPackages`、main.go `-version`、readfile の Close | other | CLI と package loader のみ。golangci は `CheckPackage` だけ使う | — | not-applicable | S | — |
| E9 | analyzer_test.go gotypesalias ループ削除 | other | テストのみ | — | not-applicable | S | — |

###### revive（v1.17.0 → v1.17.1、golangci wrapper の変更を含む）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| R1 | **golangci** pkg/goutil/env.go `Discover` を pointer receiver に | behavior | 2.14 は値 receiver で `go env GOROOT` の結果を捨てていたため、`GOROOT` 環境変数が無いホストでは `PackageLoader.prepareBuildContext` が `build.Default.GOROOT` を設定せず、revive の `importer.Default()` が**全 import を解決できなかった**（Phase 8 §4 revive #22 の「ホスト依存」の正体）。2.15 は常に GOROOT を設定するので **std は解決し、module 内の別パッケージ・依存 module は従来どおり解決しない**（実測: `fmt.Printf` 文は報告、`sub.E()` は沈黙、`exp/revue`）。2.14.0 も `GOROOT=$(go env GOROOT)` を付けると 2.15 と同じ 3 件を出す。guff の 2 箇所の「import は全部見えない」近似を「std だけ見える」に直す: unhandled-error の `callee_is_local` と unexported-return の `alias_target_is_imported` | guff-revive/src/rules/unhandled_error.rs:86-110, rules/unexported_return.rs:149-165 | **done (P9-6)** | M | golden `revive`（extended_bad.go の 23 件、unexported_return_alias.go:45）。上流 unhandled_error.go testdata は std 呼び出しを含む |
| R2 | 同上（type 依存 rule の他の観測差） | behavior | time-naming（`time.Duration`）、epoch-naming（`time.Time.Unix`）、context-keys-type の `string`（2.14 は `untyped string`）。guff は実型を持つのでこれらは既に 2.15 と同じ出力（`revive` case で guff-only 0 行）。committed golden は 2.14 の値なので、pin を上げるときは golden の regen だけでよい | rules/context_keys_type.rs:50-77, rules/time_naming.rs:57-80, rules/epoch_naming.rs:77-106 | already-matches | S | golden `revive` bad.go:50, :65, extended_bad.go:438 |
| R3 | 同上（波及の監査） | behavior | 他の type 依存 revive rule に「import が見えない」前提の近似が残っていないか（grep で明示コメントがあるのは R1 の 2 箇所だけ）。golangci 側では同じ修正で `build.Default.BuildTags = run.build-tags`（2.14 は GOROOT が空だと未設定）と cgo processor の GOCACHE 除外（2.14 は空文字で無効）も効くようになる — golangci-core 担当と要確認 | guff-revive/src/rules/*.rs 全般 / guff-lint の cgo 除外 | unsure | M | — |
| R4 | rule/add_constant.go `checkFunc` | behavior | call の引数と **Fun** を `ast.Inspect` で辿る: 複合リテラル・二項式・map/struct リテラル内の数値も報告、FuncLit は `ast.Walk(w)` で本体ごと（`go func(){ x = 1 }()` の 1 が出る）、ネストした call は自分の ignoreFuncs で、struct tag は除外。guff は直下の BasicLit と CallExpr 引数だけ | guff-revive/src/rules/add_constant.rs:41-46, 60-83 | **done (P9-6)** | M | add_constant_nested_call.go（実測 6 件すべて gcl-only）、golden extended_bad.go:450:12, 641:8 |
| R5 | golangci revive/testdata/revive_add_constant.{go,yml}（新規） | new-option | add-constant の引数 `maxLitCount` / `allowStrs` / `allowInts` / `allowFloats` / `ignoreFuncs`（v1.17.0 から存在。2.15 の golangci testdata で初めて測られる）。guff は設定を一切読まない（ファイル先頭コメントのとおり）。**P9-6 で rule 全体を移植**（文字列の回数はファイル単位：guff は package 単位で数えていた）。golden `revive-add-constant-{upstream,upstream-args,upstream-kebab,golangci}` 23/13/13/2 完全一致、`revive` の ratchet 2→削除 | add_constant.rs:13, 60-63 | **done (P9-6)** | M | golangci pkg/golinters/revive/testdata/revive_add_constant.{go,yml}, revive add_constant*.go |
| R6 | golangci revive.go `safeTomlSlice` 削除 | config-schema | viper は既に `map[string]any` を渡すので変換は no-op だった | — | not-applicable | S | — |
| R7 | rule/function_length.go `return nil`→`continue` | behavior | 空 body の関数でファイル全体を打ち切るバグの修正。guff はそのバグを意図的に再現している | guff-revive/src/rules/function_length.rs:62-90 | **done (P9-6)** | S | function_length1.go（`funLengthEmpty` 追加）、golden extended_bad.go:294:1（実測 `exp/rv2/fl`, `ac` で gcl-only） |
| R8 | rule/max_public_structs.go | behavior | exported な **struct** の TypeSpec だけ数える（2.14 は先頭 1 バイトの ToUpper 比較で interface・名前付き型・`_foo` も数え、`Éclair` は数えなかった）。guff には **rule 自体が無い**。**P9-6 で rule を新設**（`ast.IsExported` は先頭 rune の大文字判定、関数内・alias・generic も数える）。golden `revive-max-public-structs-upstream` 1/1、`revive-v1171-upstream` の shapes_mps_* | guff-revive/src/config.rs:43-126（EXTENDED_RULES に無い）, rules/mod.rs | **done (P9-6)** | S | max_public_structs.go, max_public_structs_ok.go（実測 mps.go:1:1 が gcl-only） |
| R9 | rule/struct_tag.go `useTagNbr` | behavior | asn1 / cbor / protobuf のタグ番号を key ごとの名前空間に。guff はこの 3 つのチェッカー自体が無い（「DEFERRED」）ので今回の差分は偶然一致だが、struct_tag.go の他の MATCH は落ちたまま | guff-revive/src/rules/struct_tag.rs:3-4 | needs-port | L | struct_tag.go（`CrossFormatTagNumbers` ほか）, go1.24/struct_tag.go |
| R10 | rule/unnecessary_format.go `strconv.Unquote` | behavior | 書式は**値**で `%` を探す（`"\x25d"` は黙る）。guff は引用符を剥がすだけ。**P9-6 で修正**（`guff_gostd::strconv::unquote`）。併せて**既存の乖離**: guff の関数表は上流 27 行のうち 5 行（fmt.Errorf/Printf/Sprintf/Fprintf, log.Printf）だけだった → 表を丸ごと移植（Appendf / scan 系 / log.Fatalf・Panicf / logger.* / t・b・f.* / trace.Logf） | rules/unnecessary_format.rs:134-142, util.rs:288-297 | **done (P9-6)** | S | unnecessary_format.go 末尾 4 行（実測 un.go:26:7 が guff-only） |
| R11 | rule/unnecessary_format.go `fmt.Sprintf` の文言（**既存の乖離**） | message | 上流の alternative は `` `"fmt.Sprint" or just the string itself"` `` で末尾に `"` が付く（v1.17.0 から同じ）。guff は付けていない。**P9-6 で修正** | rules/unnecessary_format.rs:31 | **done (P9-6)** | S | unnecessary_format.go（MATCH の `itself"/`、実測 un.go:27:7 の文言差） |
| R12 | rule/unnecessary_if.go | behavior | then/else の真偽値が**相補**で、代入トークンが同じ、両側の lhs が 1 個のときだけ。併せて: 上流は `exprs[0].(*ast.Ident)` で括弧を剥がさないが guff は `unparen` している（既存の乖離、未実測）。**P9-6 で移植**。併せて**既存の乖離** 3 つ: 上流は FuncDecl の body だけを walk し（package var の closure は見ない）、報告した if の部分木に降りない（`return nil`）。cond / lhs の描画は `GoFmt`（実 printer。`something+10`、FuncLit の改行）で、guff の簡易描画では一致しなかった。golden `revive-v1171-upstream` 63/63 | rules/unnecessary_if.rs:69-114 | **done (P9-6)** | S | unnecessary_if.go 末尾（実測 un.go:7:2 が guff-only） |
| R13 | rule/unnecessary_stmt.go `len(cc.List) != 1` | behavior | default だけの switch（式 / 型 switch とも）を報告しない。**P9-6 で修正** | rules/unnecessary_stmt.rs:88-93 | **done (P9-6)** | S | unnecessary_stmt.go `defaultOnly`（実測 un.go:18:9 が guff-only） |
| R14 | lint/filefilter.go | behavior | rule の `exclude` の glob とファイル名マスクを `regexp.QuoteMeta` で（`?` `|` `+` `(` なども literal）、glob も先に `\`→`/`。**P9-6 で修正**（`regex::escape`、上流 filefilter_test.go の表を unit test に） | guff-revive/src/filefilter.rs:93-145 | **done (P9-6)** | S | lint/filefilter_test.go |
| R15 | rule/*.go `Configure` の再設定リセット（17 rule） | behavior | 同じ rule インスタンスに Configure を 2 回呼んだときだけ効く。golangci は `New` で 1 回だけ（revive.go:97） | — | not-applicable | S | *_config_reset*.go |
| R16 | rule/enforce_repeated_arg_type_style.go エラー文言 | message | 設定エラーの rule 名の誤記修正。guff は revive の Configure エラーを出さない | — | not-applicable | S | — |
| R17 | `revive-lint/dots` への移行、formatter/sarif.go、revivelib | other | golangci 経路に無関係 | — | not-applicable | S | — |

###### loggercheck（v0.12.0 → v0.13.0）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| L1 | staticrules.go `nilStringerRulesetNames`、checkers/checker.go `NilStringer` | behavior | 「nil で panic しうる Stringer」は **klog と logr の ruleset だけ**。zap / slog / kitlog / custom rules では出さない | guff-style/src/loggercheck.rs:422-431 | **done (P9-5)** | S | testdata/src/a/all/example.go:47-52, customonly/example.go:37（v0.13.0 タグ。module zip に testdata/src 無し）、golden `loggercheck` stringer/v0120_stringer.go:28, 33 |
| L2 | internal/nilness/nilness.go（新規 345 行） | behavior | 自前 SSA（`ssa.NewProgram` + `SetNoReturn`: os.Exit / runtime.Goexit / log.Fatal*・Panic* / testing の Fatal 系メソッド / klog の Fatal*・Exit*）で可変長引数の値が**証明的に非 nil** なら報告しない（Alloc/FieldAddr/IndexAddr/Global/Function/MakeClosure、支配する `!= nil` 分岐、先行する deref、phi の全辺）。call は `Lparen` で SSA call に引き、同位置に 2 つあれば信用しない。SSA 構築が panic したら全部報告 | loggercheck.rs:453（nilness 無し）。基盤: guff-ssa/src/program.rs:152 `set_no_return` | needs-port | L | testdata/src/a/nilness/{example,adversarial,testing,positions_generated,external_test}.go（91 want） |
| L3 | .goreleaser.yml / release-please | other | リリース設定のみ | — | not-applicable | S | — |

###### wrapcheck（v2.12.0 → v2.13.0）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| W1 | wrapcheck.go `isError` | behavior | `typ.String() == "error"` から `types.Implements(typ, error)` へ。独自 error 型（値・ポインタ・error を埋め込む interface）を返す外部呼び出しも対象 | guff-error/src/wrapcheck.rs:144-146 `is_error_typ` | **done (P9-6)** | M | testdata/custom_error_interface/{main.go,src/ext/ext.go}（実測 9 件すべて gcl-only） |
| W2 | wrapcheck.go `isWrappableResult` | behavior | 最も内側の FuncLit / FuncDecl の、その位置（tuple なら呼び出し側 tuple の index）の結果型に `error` が代入可能なときだけ報告。`any` 結果は報告、`ext.CustomErr` / `*ext.PtrErr` / `ext.CodedErr` 結果は報告しない。**W1 と同時に入れないと過剰報告になる**（現状の沈黙は名前比較による偶然一致） | wrapcheck.rs:387-467 `check_return` | **done (P9-6)** | M | 同上（`concrete*`, `embeddedInterfaceResult`, `funcLitConcrete`） |
| W3 | wrapcheck.go `globMatcher` / compileGlobs のエラー文言 | other | gobwas/glob は v0.2.3 のまま。エラー文言は設定エラー時のみ | wrapcheck.rs:121 | not-applicable | S | — |
| W4 | （既存）wrapcheck.go の `expr.(*ast.CallExpr)` / `expr.(*ast.Ident)` | behavior | 上流は括弧を剥がさない（`return (f())` は call 扱いされず、ident でもないので文ごと終了）が guff は `unparen` している。2.15 の差分ではないが W1/W2 と同じ関数を触るので一緒に測る | wrapcheck.rs:398, 431 | **done (P9-6)** | S | — |

###### paralleltest（v1.0.15 → v1.0.16）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| P1 | pkg/paralleltest/paralleltest.go `parallelBlockingMethodIs…` | behavior | `t.Chdir` も `t.Setenv` と同じく並列不可（関数本体・t.Run 本体・range の t.Run の 3 箇所）。受信者名で照合するので別型の `Chdir` は効かない | guff-style/src/paralleltest.rs:175, 257, 292 | needs-port | S | testdata/src/t/chdir_test.go（go1.24。実測 guff-only 8 件） |

###### rowserrcheck（golangci fork c5f79b8 → e0740d5）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| X1 | passes/rowserr/rowserr.go `isErrCalledVisited` | behavior | phi の循環で無限再帰していた（2.14.0 は `fatal error: stack overflow` で落ちる、実測）。visited 集合で止めて両方の Query を報告。guff は AST 近似で再帰しないので同じ 2 件を出す | guff-error/src/rowserrcheck.rs:250-281 `handle_assign` | already-matches | S | testdata/src/a/phi_cycle.go（実測 9:30, 11:30 が一致） |
| X2 | rowserr.go Doc 文言・`isErrInstruction` 抽出・sqlPkgs の初期化位置 | other | リファクタのみ | — | not-applicable | S | — |

###### canonicalheader（golangci fork a25c71c → e18e5c5）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| C1 | analyzer.go `headerObject` 探索の削除、`isHTTPHeader` | behavior | `TypesInfo.Uses` から `net/http` の `Header` を 1 つ拾う map 順依存のゲートを撤去し、受信者型を Unalias して `net/http.Header` の Named か直接判定。`w.Header().Set(…)` だけのパッケージも報告され、「0 か 2 か乱数」も無くなる。guff のゲートと syncthing `lib/api` の allowlist は撤去、isolate fixture の methodonly.go のコメントも書き直し | guff-style/src/canonicalheader.rs:363-428 `package_has_header_typed_object`, :435-437 | **done (P9-5)** | S | golden `canonicalheader` / `-exclusions` methodonly/methodonly.go:17-21（drift +5 ×2） |
| C2 | analyzer.go `outerErr` 削除 | other | 常に nil だった | — | not-applicable | S | — |

###### golines（golangci/golines v0.15.0 → v0.16.0、formatter）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| G1 | go.mod dave/dst v0.27.3 → v0.28.0 decorator-fragment.go | behavior | 空行 2 つの並びで空行 fragment が後続の改行 fragment に上書きされて**空行が消えていた**のを修正（dst #81）。guff は golines バイナリを shell out するので、CI と手元の pin を上げる（手元の `~/go/bin/golines` は v0.15.0 / dst v0.27.3） | guff-fmt/src/golines.rs:1-13（shell out）, compat/pins.json `golines`, .github/workflows/compat.yml:94,262 | **done (P9-5)** | S | dst decorator テスト（#81）。fmt case は未作成 — 空行 2 連の前後で golines が行を折る形を足す |
| G2 | dst v0.28.0 その他（`File.GoVersion`、NoPos ガード、生成器の offset） | other | golines の出力に効かない | — | not-applicable | S | — |

###### bidichk（v0.3.3 → v0.3.4）

| # | upstream_path | kind | 要約 | guff の場所 | status | effort | upstream testdata |
|---|---------------|------|------|-------------|--------|--------|-------------------|
| B1 | go.mod（go 1.26、x/tools v0.51） | other | コード差分なし | — | not-applicable | S | — |

##### PR 分割

| PR | 内容 | 行 | effort | fixture / ゲート |
|----|------|----|--------|------------------|
| A | errcheck v1.30: range-over-func、channel receive、型変換、errors.AsType | E1–E5 | M | golden `errcheck` / `errcheck-blank` に iter.go・chan.go・errors_astype.go（go1.26 module）・`var _ error = (*T)(nil)` を足す。errcheck は golden drift 0 なので**足さないと何も測られない** |
| B | revive: golangci の GOROOT 修正に追従（std は解決、module 内は未解決） | R1（+R3 の監査） | M | golden `revive` の unhandled-error 23 件・unexported-return 1 件。std / module 内 / 依存 module の 3 形を 1 case に |
| C | revive 小修正まとめ | R7, R8, R10–R14 | S×7（計 M） | function_length1.go、max_public_structs{,_ok}.go、unnecessary_{format,if,stmt}.go、filefilter の単体テスト |
| D | revive add-constant: 引数と nested 走査 | R4, R5 | M | add_constant_nested_call.go、golangci revive_add_constant.{go,yml} |
| E | canonicalheader ゲート撤去 + paralleltest Chdir | C1, P1 | S | golden canonicalheader ×2（regen）、chdir_test.go |
| F | wrapcheck isError / isWrappableResult（W4 も測る） | W1, W2, W4 | M | custom_error_interface/ を golden case に |
| G | loggercheck ruleset ゲート | L1 | S | golden loggercheck を regen、klog / logr の fixture を足す（現 fixture は slog だけで、ゲート後は全部沈黙して何も測らない） |
| H | loggercheck nilness（自前 SSA + noReturn） | L2 | L | v0.13.0 タグの testdata/src/a/nilness 全体（91 want）を上流 testdata ごと |
| I | golines pin を v0.16.0 に | G1 | S | compat/pins.json、fmt case 1 本 |
| 後回し | revive struct-tag の asn1 / cbor / protobuf チェッカー | R9 | L | struct_tag.go 全体 |

件数: needs-port 22、already-matches 3、unsure 2、not-applicable 14（計 41 行）。
