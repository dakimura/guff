# guff `go/types` 移行計画書（まぬけなLLM向け・ステップバイステップ）

> この文書は **あなた（作業するLLM）への指示書** です。
> あなたは賢くないかもしれません。だから **自分で判断せず、この文書のルールと手順に従ってください**。
> 1回の作業（1ステップ）で **触ってよいファイルは原則1〜2個まで** です。それ以上は触らないでください。

---

## 0. 最初に必ず読むこと（作業の鉄則 / 守らないと壊れます）

以下は **例外なく守るルール** です。迷ったらここに戻ってください。

1. **1ステップ＝ファイル1〜2個まで。** 「ついでに」他のファイルを直さない。やり残しは次のステップに回す。
2. **Goのソースが唯一の正解（source of truth）。** 自分の記憶でGoの仕様を書かない。必ず下記のGoファイルを `Read` してから移植する。
   - Goソースの場所: `/Users/dakimura/sdk/go1.26.4/src/cmd/compile/internal/types2/`
   - 移植元は **`go/types` ではなく `cmd/compile/internal/types2`** です。間違えないこと。
3. **既存のコードのまねをする。** 新しいファイルを書く前に、似た既存ファイル（例: `under.rs`, `predicates.rs`）を `Read` して、import の書き方・関数シグネチャの形・コメントの付け方をまねる。
4. **ポインタ `*T` は ID に変換する。** Goの `*Type` は `TypeId`、`*Var`/`*Func` などObjectは `ObjectId`、`*Scope` は `ScopeId`、`*Package` は `PackageId`。`nil` ポインタは `Option<...>` の `None`。詳細は §5。
5. **作業の最後に必ずビルドとテストを通す。** 通らないまま「完了」と言わない。コマンドは §4 にある。
6. **テストが落ちている／コンパイルが通らないなら、それは「未完了」。** 勝手に `#[ignore]` や `todo!()` で隠さない（例外は §3 のdeferralルールに従うときだけ）。
7. **コミットメッセージは決まった形式で書く**（§4参照）。`git push` は **しない**（ユーザーがやる）。
8. **わからない関数・移植できない部分は「deferral（後回し）」として明示する。** 黙って省略しない。やり方は §3。

---

## 1. プロジェクトは何か（ゴール）

- **guff** は、Goの標準ライブラリ（`go/token` → `go/ast` → `go/constant` → `go/types` …）を Rust に移植するプロジェクト。
- 最終ゴールは **golangci-lint を Rust で書き直すための土台** を作ること。
- いま移植しているのは **`go/types`（型チェッカ）** で、これが一番大きくて難しい。
- リポジトリのパス: `/Users/dakimura/projects/src/github.com/dakimura/me/projects/guff`
- 作業対象のクレート: **`crates/guff-types`**（lib名は `guff_types`）

### クレート構成（既にあるもの）
```
crates/
  guff-ast/          (lib: guff)          … token, scanner, ast, parser  ← 移植済み
  guff-constant/     (lib: guff_constant) … go/constant                  ← 移植済み
  guff-gover/        guff-goversion/  guff-version/                  ← 移植済み
  guff-types-errors/ (lib: guff_types_errors) … エラーコード Code enum   ← 移植済み
  guff-types/        (lib: guff_types)    … go/types  ← ★ここで作業する★
```
- クレート間の依存は `Cargo.toml` の `path = "../..."` で書く。
- `guff-types` は既に `guff-ast`, `guff-constant`, `guff-types-errors` に依存している。

---

## 2. 現在の進捗（どこまで終わっているか）

**型チェッカの「土台（structural層）」はほぼ完成。残りは「Checkerエンジン本体（Tier 4）」がメイン。**

- テスト数: **750 tests passing**（chunk81時点、`cargo test -p guff-types`）。作業のたびにこの数を増やしていく。
- 進捗率: types2 の非テストLOCのうち **約45%** 移植済み（構造層）。残りはCheckerエンジン約8.8K LOC。

### 移植済みのもの（chunk 1〜17）
- **型の種類（全部）**: Basic, Array, Slice, Pointer, Map, Chan, Tuple, Struct, Signature, Interface, Union, Named, Alias, TypeParam
- **オブジェクトの種類**: Var, Func, TypeName, Const, Nil, Builtin（**まだ無い: Label, PkgName**）
- **格納庫（arena）**: TypeArena/ObjectArena/ScopeArena/PackageArena と TypeId/ObjectId/ScopeId/PackageId
- **その他構造**: Scope, Package, ObjSet, ObjectMeta（parent/pkg/pos/order/scope_pos）
- **型集合の代数**: typeterm, termlist, typeset
- **述語**: identical（構造的比較）, comparable, is_basic系, default_type など（`predicates.rs`）
- **Universe**: `init_universe_full()` が全部入りのUniverseを返す
- **ジェネリクス**: Context, subst, instantiate, infer, unify
- **補助**: under（under_is/all/common_under）, selection, validtype, lookup
- **オペランド層**: operand, conversions（convertible_to）, assignments（assignable_to）
- **型の文字列化**: typestring（type_string/signature_string）

### まだ手つかず（これから作る ＝ あなたの仕事）
- **Checkerエンジン本体（Tier 4）**: `check.go`, `errors.go`, `format.go`, `api.go`, `resolver.go`, `decl.go`, `typexpr.go`, `expr.go`, `stmt.go`, `call.go`, `builtins.go`, `index.go`, `const.go`, `recording.go`, `assignments.go`（Checker側）, `signature.go`/`interface.go`/`struct.go`/`named.go` のChecker部分
- **Tier 5（補助）**: `mono.go`, `cycles.go`, `initorder.go`, `labels.go`, `range.go`, `return.go`, `literals.go`, `sizes.go`, `api_predicates.go`, `util.go`, `version.go`

完全な残りファイル一覧と順番は §7 にある。

---

## 3. 「deferral（後回し）」のルール

移植中、**まだ移植していない別の関数に依存していて書けない部分**が必ず出てくる。そういうときは：

1. **黙って消さない。** 代わりに「あとで埋める」マークを残す。
2. Rust側のやり方:
   - 関数まるごと後回し → その関数は **書かない**。代わりにファイル末尾に
     ```rust
     // ===== DEFERRED (forward pointer) =====
     // Go: Checker.foo (check.go:123) — needs bar() which is not ported yet.
     // TODO(chunk-NN): port when bar() lands.
     ```
     のようなコメントを書く。
   - 関数の一部だけ後回し → その箇所に
     ```rust
     // DEFERRED: Go calls check.bar() here; not ported yet. Using <安全なデフォルト> for now.
     ```
     と書き、**安全なデフォルト**（`false` / `None` / 空Vec / `Typ[Invalid]`）で代用する。
   - 既に他のchunkでやっている「クロージャ注入」パターン（`implements`/`representable` を `&dyn Fn(...) -> bool` で受け取る）も有効。既存の `conversions.rs` / `assignments.rs` を見てまねる。
3. **deferral を作ったら §8 の表に1行追記する。** これで後から回収できる。

---

## 4. 1ステップの固定作業手順（毎回これをやる）

> **重要: この手順を毎ステップ、上から順にやる。飛ばさない。**

1. **どのステップをやるか決める。** §7 のステップ一覧で「未着手」の一番上を選ぶ。
2. **Goソースを読む。** そのステップに書いてあるGoファイル・関数を `Read` する。
   例: `/Users/dakimura/sdk/go1.26.4/src/cmd/compile/internal/types2/decl.go`
3. **既存の似たRustファイルを読む。** §7に「参考にするファイル」が書いてある。書き方をまねるため。
4. **Rustコードを書く。** §5の翻訳ルールに従う。新規ファイルなら `src/xxx.rs` を `Write`。
5. **`lib.rs` にモジュール登録する。** 新しいファイルを作ったら `pub mod xxx;` と、必要なら `pub use xxx::{...};` を追記する（これも1ファイルとして数える、許容範囲）。
6. **テストを書く。** `tests/xxx.rs` を新規作成し、そのステップの「テスト指示」に書かれたケースを書く。
7. **ビルド＆テスト:**
   ```bash
   . "$HOME/.cargo/env"   # cargo はPATHに無いので毎回これを最初に実行
   cd /Users/dakimura/projects/src/github.com/dakimura/me/projects/guff
   cargo build -p guff-types
   cargo test  -p guff-types
   ```
   - **エラーが出たら直す。** 通るまでこのステップを抜けない。
   - テスト数が前より減っていたらどこか壊した。直す。
8. **deferral があれば §8 の表に追記する。**
9. **コミットする**（pushはしない）:
   ```bash
   cd /Users/dakimura/projects/src/github.com/dakimura/me/projects/guff
   git add -A
   git commit -m "guff: <やったことを1行で> (chunk NN)"
   ```
   コミットメッセージ本文の最後に、別行で次を必ず付ける:
   ```
   Co-authored-by: Cursor <cursoragent@cursor.com>   ```
10. **このステップを「完了」にする**（§7のチェックボックスを `[x]` にして、その変更もコミットに含めてよい）。

---

## 5. Go → Rust 機械的翻訳ルール集（暗記する）

このプロジェクトは「arenaパターン」を使う。Goのポインタを全部IDに置き換えるのが肝。

| Go | Rust |
|----|------|
| `*Type`（型へのポインタ） | `TypeId` |
| `*Var`, `*Func`, `*TypeName`, `*Const`（オブジェクト） | `ObjectId` |
| `*Scope` | `ScopeId` |
| `*Package` | `PackageId` |
| `nil`（ポインタが空） | `Option<...>` の `None` |
| `func foo(t *Type) ...` | `fn foo(arena: &TypeArena, t: TypeId) -> ...`（読むだけなら `&`、作る/変えるなら `&mut`） |
| メソッド `func (t *Type) Underlying() Type` | 既に `TypeId::underlying(&self, arena)` がある。新規は free function `type_underlying(arena, t)` でもメソッドでもよい（既存にならう） |
| `map[*Object]Foo` | `HashMap<ObjectId, Foo>` |
| `[]*Type` | `Vec<TypeId>` |
| 構造体フィールドの代入 `t.field = x` | `arena.get_mut(t)` で取り出して書き換え。ただしborrowエラーに注意（§5.1） |
| `constant.Value` | `guff_constant::Value` |
| `syntax.Expr` / `syntax.Stmt` / `*syntax.Name` | `guff::ast::Expr` / `Stmt` / `Ident`（`crates/guff-ast/src/ast.rs` にある） |
| エラーコード（`. "internal/types/errors"` の `InvalidFoo`） | `guff_types_errors::Code::InvalidFoo` |
| `panic("assertion failed")` の `assert(p)` | `assert!(p)`（Rust標準マクロ）。Goの `assert()` ヘルパは `assert!` に置き換える |
| `Typ[Invalid]` | universe/basicテーブルの Invalid 型の `TypeId`（`init_universe` 由来。`basic.rs` 参照） |

### 5.1 borrow checker の落とし穴（重要・まぬけがハマる所）
Rust では「arena から `&mut` で借りている最中に、同じarenaをもう一度借りる」とコンパイルエラーになる。Goでは平気だったコードがそのままだと通らない。

**対処パターン（既存コードでも多用されている）:**
- 必要な値を **先にローカル変数へコピー（snapshot）してから** 再帰やループに入る。
  ```rust
  // ダメ: arena.get(t) で借りたまま中で identical(arena, ...) を呼ぶ
  // 良い: 先にフィールドを取り出す
  let elem = match arena.get(t) { TypeData::Slice(s) => s.elem, _ => unreachable!() };
  let r = some_fn(arena, elem); // ここでは arena は自由
  ```
- `TypeId`/`ObjectId` は `Copy`（コピーが軽い）。気軽にローカルにコピーしてよい。
- 既存の `predicates.rs::identical` が「snapshotしてから再帰」の見本。困ったら読む。

### 5.2 命名規則
- 1つのGoファイル `foo.go` → 1つのRustモジュール `foo.rs`（名前が予約語なら `r#struct` のようにする。既存例: `struct.rs`）。
- コンストラクタは free function `new_xxx(...)`。
- アクセサは `xxx_field(arena, id)` の free function、もしくは `Id::method(&self, arena)` のメソッド。**既存ファイルの流儀に合わせる。**
- 公開するものは `lib.rs` に `pub use` を足す。

---

## 6. ★最重要★ Checker 構造体の設計（ここだけは自分で考えず、この設計に従う）

`check.go` の `Checker` 構造体は、これから作る全Checkerファイルの土台。**Goのポインタmapを全部ID keyに変える**のがポイント。以下の設計で `src/check.rs` を作る。

### 6.1 Checker が持つもの（フィールド）
```rust
pub struct Checker {
    // ---- arena（Goではグローバルだったが、ここではCheckerが所有する）----
    pub types: TypeArena,
    pub objects: ObjectArena,
    pub scopes: ScopeArena,
    pub packages: PackageArena,

    // ---- universe（init_universe_full から取り込む。基本型テーブルなど）----
    pub universe: Universe,          // 既存の Universe 構造体を中に持つ
    pub typ: Vec<TypeId>,            // Basicテーブル（Typ[Invalid] などの索引用）

    // ---- パッケージ情報（NewChecker で初期化、生存期間は Checker と同じ）----
    pub conf: Config,                // §6.2
    pub pkg: PackageId,              // 検査対象パッケージ
    pub info: Info,                  // 結果の記録先（§6.3）
    pub ctxt: Context,               // インスタンス重複排除（既存 context.rs）
    pub next_id: u64,                // TypeParam の一意ID採番（最初は1）

    // Goの map[Object]*declInfo 等 → ID keyにする
    pub obj_map: HashMap<ObjectId, DeclInfo>,   // パッケージレベル宣言の情報
    pub obj_list: Vec<ObjectId>,                // obj_map のソート済みキー

    // ---- ファイル検査中だけ有効な情報 ----
    pub files: Vec<guff::ast::File>,
    pub imports: Vec<ObjectId>,                 // PkgName オブジェクト（後で PkgName を足す）
    pub methods: HashMap<ObjectId, Vec<ObjectId>>, // TypeName → メソッド(Func)群
    pub untyped: HashMap</*expr id*/ u32, ExprInfo>, // 型未確定の式（§6.4）
    pub delayed: Vec<Action>,                   // 遅延アクション（§6.5）
    pub obj_path: Vec<ObjectId>,                // サイクル検出用の依存パス
    pub used_vars: HashMap<ObjectId, bool>,     // 使用済み変数
    pub first_err: Option<String>,              // 最初に出たエラー
    pub errors: Vec<TypeCheckError>,            // 集めたエラー（§6.6。Goは即時報告だがここでは溜める）

    // ---- いま検査中のオブジェクトの「環境」（Goの埋め込み environment）----
    pub env: Environment,                       // §6.7
}
```

> **注意（まぬけ向け）**: Goの `Checker` は `TypeArena` 等を持っていない（Goはグローバル/GC）。**Rust版だけが arena を Checker に入れる。** これがこの移植で最大の構造的違い。これまでの chunk では関数に arena を引数で渡していたが、Checker登場後は「Checkerのメソッドにして `self.types` 等を使う」形に寄せていく。**ただし既存の free function は壊さない**（Checkerメソッドから呼べばよい）。

### 6.2 `Config`（`api.go` 由来。最小から始める）
```rust
#[derive(Default)]
pub struct Config {
    pub go_version: String,         // 例 "go1.26"
    pub disable_unused_import_check: bool,
    pub trace: bool,                // デバッグ出力（基本 false）
    // 足りなくなったら api.go を見て増やす。最初はこれだけでよい。
}
```
- Goの `Config` には `Importer`, `Sizes`, `Error func(error)` などがあるが、**最初は省略（deferral）**。必要になった時に足す。

### 6.3 `Info`（`api.go` 由来。型チェック結果の記録先）
```rust
#[derive(Default)]
pub struct Info {
    pub types: HashMap<u32 /*expr id*/, TypeAndValue>, // 式→(型,値)
    pub defs:  HashMap<u32 /*ident id*/, Option<ObjectId>>, // 定義された識別子→Object
    pub uses:  HashMap<u32 /*ident id*/, ObjectId>,    // 使われた識別子→Object
    // scopes, implicits, selections, instances は後で足す（deferral）
}
```
- **問題**: GoはExpr/Identをポインタでキーにする。Rust版ではASTノードに一意IDが要る。
  - まず `crates/guff-ast/src/ast.rs` を `Read` して、Expr/Ident に既にIDやPosがあるか確認する。
  - 無ければ「Posを使う」「ノードにidフィールドを足す」等が必要 → **これはstep 18bで別途検討する**（§7参照）。それまでは `Info` の中身は空のままでよい（recordは no-op）。

### 6.4 `ExprInfo`（`check.go:51` の `exprInfo`）
```rust
pub struct ExprInfo {
    pub is_lhs: bool,
    pub mode: OperandMode,           // 既存 operand.rs
    pub typ: TypeId,                 // *Basic だが TypeId で持つ
    pub val: Option<guff_constant::Value>,
}
```

### 6.5 `Action`（`check.go:113` の `action`。遅延実行）
- Goは `f func()` クロージャを溜める。Rustでは `Box<dyn FnOnce(&mut Checker)>` で持つのが素直。
  ```rust
  pub struct Action {
      pub version: String, // goVersion
      pub f: Box<dyn FnOnce(&mut Checker)>,
  }
  ```
- **borrow地獄に注意**: クロージャが `&mut Checker` を取る形にして、`later()` は `self.delayed.push(...)`、`process_delayed()` は一旦 `std::mem::take(&mut self.delayed)` してから順に呼ぶ。

### 6.6 エラーの扱い（Goと変える方針）
- Goは `check.errorf(...)` で**即座に**報告（`firstErr` に記録 or `conf.Error` 呼び出し）。
- Rust版は **`self.errors: Vec<TypeCheckError>` に溜める**方針にする（テストで検証しやすい）。
  ```rust
  pub struct TypeCheckError {
      pub pos: u32,            // syntax.Pos の代わり（最初は 0=unknown でよい）
      pub code: Code,          // guff_types_errors::Code
      pub msg: String,
  }
  ```
- `self.error(pos, code, msg)` / `self.errorf(pos, code, fmt, args)` を `errors.rs` で実装（§7 step 19）。

### 6.7 `Environment`（`check.go:60` の `environment`）
```rust
#[derive(Default)]
pub struct Environment {
    pub scope: Option<ScopeId>,      // いまの最上位スコープ
    pub version: String,             // 受理中のGo言語バージョン
    pub iota: Option<guff_constant::Value>, // const宣言中のiota値
    pub sig: Option<TypeId>,         // 関数本体検査中ならそのSignature
    pub decl: Option<usize>,         // いま検査中の宣言（obj_mapのindex等。最初は省略可）
    // 残り（errpos, in_tparam_list, has_label など）は必要になったら足す
}
```

---

## 7. ステップ一覧（上から順にやる）

各ステップは **ファイル1〜2個** に収まるよう細かく割ってある。各ステップに:
- **Goソース**（読むファイル）
- **作るRustファイル**
- **参考にする既存ファイル**
- **関数ごとのロジック（日本語）**
- **deferral（後回しにしてよいもの）**
- **テスト指示**
が書いてある。完了したら `[ ]` を `[x]` にする。

> **注記**: chunk 1〜17 は完了済み。次は **chunk 18** から。

---

### 🟦 Tier 4-A: Checker の骨組み（最優先・他の全部の土台）

#### [x] Step 18a — `Config` / `Info` / `TypeCheckError` の器を作る
- **Goソース**: `api.go`（`Config`, `Info`, `TypeAndValue` の定義部分だけ。`Read` して構造を見る）
- **作るRustファイル**: `src/api.rs`（新規）
- **参考**: `operand.rs`（`TypeAndValue` に似た `Operand` がある）, `lib.rs`（pub useの足し方）
- **ロジック（日本語）**:
  - `Config` 構造体: §6.2 の最小版をそのまま書く。`#[derive(Default)]`。
  - `Info` 構造体: §6.3 の最小版。中身のHashMapは空でよい。`#[derive(Default)]`。
  - `TypeAndValue` 構造体: `{ mode: OperandMode, typ: TypeId, val: Option<Value> }`。GoのbitフラグメソッドはまだいらないのでDEFERRED（コメントを残す）。
  - `TypeCheckError`: §6.6 の構造体。
- **deferral**: `Importer`, `Sizes`, `conf.Error` クロージャ, `Info` の scopes/selections/instances, `TypeAndValue` のフラグ系メソッド。→ §8に記録。
- **テスト**: `tests/api.rs` 新規。`Config::default()` と `Info::default()` が作れること、`TypeCheckError` が作れることだけ確認（コンパイルが通れば実質OK）。
- **コミット**: `guff: add Config/Info/TypeAndValue scaffolding (chunk 18a)`

#### [x] Step 18b — ASTノードの一意ID戦略を決める（調査＋小実装）
- **目的**: `Info.types`/`defs`/`uses` はExpr/Identをキーにする。Rustではキーになる一意IDが要る。
- **Goソース**: 不要。代わりに **`crates/guff-ast/src/ast.rs` を `Read`** して、Expr/Ident が `Pos` を持つか、idフィールドがあるか確認する。
- **作る/触るファイル**: 調査結果しだい。次の2択（**賢い判断が要るが、迷ったらAを選ぶ**）:
  - **A案（おすすめ・簡単）**: 当面 `Info` への記録は **no-op（何もしない）** にする。キー問題は実際に必要になる `expr.go` 移植時（ずっと先）まで先送り。→ この場合 step 18b は「`Info` に空の `record_type_and_value()` メソッドを足し、中身は `// DEFERRED` コメントだけ」にする。`src/api.rs` 1ファイルのみ変更。
  - **B案（難しい・やらない方が無難）**: ASTノードにidを付与する。`guff-ast` を触ることになり影響範囲が大きい。**まぬけは選ばないこと。**
- **テスト**: 既存テストが落ちないこと。
- **コミット**: `guff: defer AST-node identity for Info recording (chunk 18b)`

#### [x] Step 18c — `Checker` 構造体と `new_checker` / arena所有
- **Goソース**: `check.go`（`Checker` struct: 137行目〜, `NewChecker`: 282行目〜）
- **作るRustファイル**: `src/check.rs`（新規）
- **参考**: `universe.rs`（`init_universe_full` がarenaをまとめて返す形）, `arena.rs`
- **ロジック（日本語）**:
  - `Checker` 構造体を §6.1 の通り定義する。**ただし最初から全フィールドを入れず、コンパイルが通る最小集合から始めてよい**（types/objects/scopes/packages/universe/typ/conf/pkg/info/errors/env/next_id/delayed/obj_map/obj_list）。残りは使う段になって足す。
  - `Checker::new(conf: Config) -> Checker`:
    - `init_universe_full()` を呼んでUniverse一式（4つのarena＋typテーブル）を取り込む。
    - 検査対象パッケージを `packages.alloc(new_package(...))` で作り、その `PackageId` を `pkg` に入れる。
    - `obj_map`/`obj_list`/`errors`/`delayed` は空で初期化。`next_id = 1`。`env = Environment::default()`。
    - **borrow注意**: `init_universe_full` が返すarenaを「ムーブ」してCheckerのフィールドに入れる。Universeがarenaを内部に持っているなら、その所有権の出し方を `universe.rs` を読んで確認する。**もしUniverseがarenaを内包して取り出せない構造なら、Checkerは `universe: Universe` を1個持ち、`self.universe.types` のようにアクセスする形にする**（この方が安全。こちらを推奨）。
  - `later(&mut self, f)`: `self.delayed.push(Action{ version: self.env.version.clone(), f: Box::new(f) })`。
  - `process_delayed(&mut self, top: usize)`: `let actions = self.delayed.split_off(top); for a in actions { (a.f)(self); }`（Goのloop。新規pushされる分も処理する点に注意 — split_offだと取りこぼすので、`while i < self.delayed.len()` のindexループにする方が忠実。**Goの574行目の実装をまねる**）。
  - `push`/`pop`（obj_path操作）: そのまま `Vec::push`/`pop`。
- **deferral**: `handleBailout`（panic回収）, `initFiles` のバージョン処理, `_aliasAny` 状態機械, `cleanup`/`cleaners`, `pkgPathMap`/`seenPkgMap`, `recordTypeAndValueInSyntax`系（StoreTypesInSyntax）。全部コメントで forward pointer を残す。
- **テスト**: `tests/check.rs` 新規。`Checker::new(Config::default())` が作れ、`self.typ[Invalid]` 相当の基本型が引けること、`later`/`process_delayed` が順番に呼ばれること（カウンタを増やすクロージャを2個積んで確認）。
- **コミット**: `guff: add Checker struct + new_checker + later/process_delayed (chunk 18c)`

---

### 🟦 Tier 4-B: エラー報告

#### [x] Step 19 — `errors.rs`（エラー収集の仕組み）
- **Goソース**: `errors.go`（特に `assert`, `error_`, `newError`, `addf`, `report`, `Checker.error`, `Checker.errorf`）と `format.go`（`sprintf`, `check.sprintf`）
- **作るRustファイル**: `src/errors.rs`（新規）。`format.go` 由来の `sprintf` も小さいのでここに同居させてよい（2ファイル相当だが密接なので可）。
- **参考**: `under.rs` の `TypeError`/`type_errorf`（既にある簡易フォーマッタ）, `typestring.rs`（型名の文字列化に使う）
- **ロジック（日本語）**:
  - `Checker::error(&mut self, pos: u32, code: Code, msg: &str)`: `TypeCheckError` を作り、`self.first_err` が空なら入れ、`self.errors.push(...)`。
  - `Checker::errorf(&mut self, pos, code, fmt, args)`: Rustには可変長printfが無いので、**呼び出し側で `format!()` して `&str` を渡す方針にする**。つまり `errorf` は作らず `error` に集約してよい（まぬけが楽な方）。Goの `check.errorf(pos, Code, "x %s", y)` は Rust では `self.error(pos, Code, &format!("x {}", y))` に機械的に変換する。**この変換ルールを徹底する。**
  - `sprintf` 相当: 型名を出すときは `typestring::type_string(...)` を使う。`%s` に型が来る箇所はこれで置換。
  - `assert!` はRust標準マクロを使う（Goの `assert(p)` → `assert!(p)`）。
- **deferral**: `error_` の複数行（`desc []errorDesc`）、`soft` フラグ、`continuation` エラー、`runtime.Caller` による位置情報、`error.report` の重複抑制。最初は「1エラー＝1メッセージ」で十分。
- **テスト**: `tests/errors.rs` 新規。`Checker` に `error()` を2回呼んで `self.errors.len()==2`、`first_err` が1個目であること。
- **コミット**: `guff: add Checker error collection (errors.rs) (chunk 19)`

---

### 🟦 Tier 4-C: 既存クロージャの「本物化」（deferral回収）

ここで chunk 11/15/16 で `&dyn Fn(...) -> false` で誤魔化していた `implements` / `representable` / `conversion` を Checker のメソッドとして本実装する。**1ステップ1メソッド** に割る。

#### [x] Step 20a — `Checker::implements`（インターフェース充足判定）
- **Goソース**: `lookup.go` の `missingMethod`, `(*Checker).implements`, `MissingMethod`
- **作る/触るRustファイル**: `src/check_lookup.rs`（新規。check本体を太らせない）。`lib.rs` に登録。
- **参考**: `lookup.rs`（構造的lookupは移植済み。それを呼ぶ）, `typeset.rs`
- **ロジック（日本語）**:
  - `Checker::implements(&mut self, v: TypeId, t: TypeId, static_: bool) -> Option<MissingMethodResult>`:
    - `t` のメソッド集合（typeset）を取り、`v` が全部持つか `lookup_field_or_method` で調べる。
    - 1個でも欠けていれば「どのメソッドが無い／シグネチャ違い」を返す。全部あれば `None`（充足）。
    - Goの `missingMethod` の分岐（メソッド無し / 受信者ポインタ必要 / シグネチャ不一致）をそのまま写す。
  - 完了したら、`conversions.rs` / `assignments.rs` の呼び出し側が `&|_,_| false` 渡しだった所を **Checkerメソッド経由に差し替えられる**ようになる（差し替え自体は別step 20cで）。
- **deferral**: `hasAllMethods`（制約のメソッド集合チェック、infer.rsから参照）も近いのでここで一緒に or 次stepで。エラーメッセージ整形（`funcString`）は簡易でよい。
- **テスト**: `tests/check_lookup.rs` 新規。空interfaceは何でも満たす、1メソッドinterfaceをそのメソッドを持つNamed型が満たす／持たない型は満たさない、を確認。
- **コミット**: `guff: implement Checker::implements (chunk 20a)`

#### [x] Step 20b — `Checker::representable`（未型付き定数の表現可能性）
- **Goソース**: `expr.go` の `representable`, `representableConst`, `implicitTypeAndValue`（の表現可能性部分）, `const.go` の補助
- **作る/触るRustファイル**: `src/check_expr_const.rs`（新規）。`lib.rs` に登録。
- **参考**: `guff_constant` クレート（定数の比較・変換API）, `basic.rs`
- **ロジック（日本語）**:
  - `Checker::representable(&mut self, x: &mut Operand, typ: TypeId)`: `x`（定数）が型 `typ` で表せるか判定。表せれば `x.val` を丸めて確定、表せなければ `self.error(...)`。
  - `representable_const(val, typ) -> bool`: 整数なら範囲チェック、floatなら丸め、complexなど。`guff_constant` の関数を使う。Goの `representableConst` の数値分岐を忠実に。
- **deferral**: `updateExprType`（式の型の再設定）はInfo記録に絡むので後回し。
- **テスト**: `tests/check_expr_const.rs`。`100` は `int8` で表現可、`1000` は `int8` で不可、などをConstオペランドで確認。
- **コミット**: `guff: implement Checker::representable (chunk 20b)`

#### [x] Step 20c — 注入クロージャを本物に差し替え
- **触るRustファイル**: `conversions.rs` と `assignments.rs` の呼び出し口（**2ファイルまで**）。
- **ロジック**: これまで `assignable_to(..., &|_,_| false, ...)` のように渡していた所を、Checker側で `|v, t| self.implements(...).is_none()` 等のクロージャを作って渡す薄いラッパ（`Checker::assignable_to` / `Checker::convertible_to`）を `check.rs` か `check_expr.rs` に追加。**free functionは消さない**（Checkerメソッドが内部で呼ぶ）。
- **deferral**: §8の該当行を「回収済み」に更新。
- **テスト**: 既存 `tests/conversions.rs` / `tests/assignments.rs` が引き続き通る＋Checker経由のラッパで1ケース。
- **コミット**: `guff: wire real implements/representable into conv/assign (chunk 20c)`

---

### 🟦 Tier 4-D: 型式の検査（typexpr）と宣言（decl）

#### [x] Step 21 — `typexpr.rs`（型を表す式 → TypeId） — 21a (ident/paren/pointer/slice) + 21b (array(literal len)/map/chan) + **21c/21d=chunk33 (struct/interface/func型式) done (2026-06-19)**. 残: generic instantiation(`T[...]`)/pkg修飾型(`pkg.T`) は各専用chunkで（DEFERRED）。
  - **chunk33a `struct_check.rs::struct_type`**: struct{...}型式(named/embedded field, tag, ObjSetで重複名検出)。typexprにStructType+FuncType(func_type再利用)配線。struct型/struct literal(D25)/field access が通る。
  - **chunk33b `interface_check.rs::interface_type`**: interface{...}型式(explicit method + embedded interface + `~T|U`制約union via parse_union)。type set即時計算。typexprにInterfaceType配線。interface宣言/interface充足(implements)が通る。**Deferral(D26)**: interface method recv未設定(sig.recv=None; implements/missingMethodはrecv無視で比較するので安全)、sortMethods省略、method型パラメータ拒否。
- **Goソース**: `typexpr.go`（`(*Checker).typ`, `typInternal`, `definedType`, `genericType`, `typeList`, `arrayLength`, `ident`（型名解決部分））
- **作るRustファイル**: `src/typexpr.rs`（新規）。大きいので **2サブステップに割ってよい**（21a: ident解決＋基本ケース、21b: array/map/chan/struct/interface/func/generic）。
- **参考**: `typestring.rs`（逆方向）, `universe.rs`（型名lookup）, `struct.rs`/`interface.rs`/`signature.rs`（型の作り方）
- **ロジック（日本語）**:
  - `Checker::typ(&mut self, e: &Expr) -> TypeId`: AST式を受け取り型を返す入口。`typ_internal` を呼び、結果を `Info` に記録（記録はno-opでよい§18b）。
  - `typ_internal(e)`: 式の種類でmatch:
    - `Ident`（`int`, 自前の型名）→ スコープを引いて、その Object が TypeName なら型を返す。未定義なら `error` して `Typ[Invalid]`。
    - `*ast::ArrayType` → 要素型を再帰 `typ`、長さを `array_length` で評価、`new_array`/`new_slice`。
    - `*ast::MapType` → key/value再帰、`new_map`。
    - `*ast::ChanType` → 方向＋要素、`new_chan`。
    - `*ast::StructType` → フィールドを集めて `new_struct`（重複フィールドチェックは `objset`）。
    - `*ast::InterfaceType` → `new_interface_type`＋`interface_compute_typeset`。
    - `*ast::FuncType` → `signature`（step 23で作る `func_type` を使う or 簡易版）。
    - `*ast::SelectorExpr`（`pkg.T`）→ パッケージスコープ解決。**importが未実装ならDEFERRED**。
    - `*ast::IndexExpr`/`IndexListExpr`（ジェネリックインスタンス化 `T[int]`）→ `instantiate`（既存）。
  - `array_length(e) -> Option<i64>`: 定数式を評価。`expr.go` の定数評価が要るので、**最初は整数リテラルだけ対応し、それ以外はDEFERRED**。
- **deferral**: パッケージ修飾型（import未実装）、複雑な配列長定数式、`[...]T`（要素数推論、composite litが要る）。
- **テスト**: `tests/typexpr.rs`。`int` ident→Basic、`[]int`→Slice、`map[string]int`→Map、`*int`→Pointer、`struct{x int}`→Struct を、ASTを手で組んで（or 小さくパースして）確認。**ASTの組み方は `tests/operand.rs` がExprを使っているので参考にする。**
- **コミット**: `guff: port typexpr.go (type expressions) (chunk 21)`

#### [x] Step 22 — `resolver.rs`（パッケージレベルのオブジェクト収集）
- **Goソース**: `resolver.go`（`collectObjects`, `declarePkgObj`, `(*Checker).declare`, `importDecl`/`varDecl`/etc. のObject作成部分）
- **作るRustファイル**: `src/resolver.rs`（新規）
- **参考**: `scope.rs`（insert/lookup）, `decl.go`（DeclInfo）
- **ロジック（日本語）**:
  - `DeclInfo` 構造体（`decl.go` 由来）をここか `decl.rs` に定義: 宣言の種類・スコープ・型式・初期化式・依存集合を持つ。
  - `Checker::collect_objects(&mut self)`: 各ファイルの各トップレベル宣言を走査し、`Const`/`Var`/`TypeName`/`Func` のObjectを作ってパッケージスコープに `scope_insert`、`obj_map` にDeclInfoを登録。
  - `declare_pkg_obj`: 名前の重複チェック→スコープへ挿入→obj_map登録。
  - import宣言: **importerが無いのでDEFERRED**（importはエラーにせず空で飛ばす or 簡易処理）。
- **deferral**: import全般、dot-import、init関数の特別扱い、メソッドの型への割り当て（`methods` map）の一部。
- **テスト**: `tests/resolver.rs`。`var x int; const c = 1; func f(){}` を含む小さいFileをパースし、collect後にパッケージスコープに x/c/f が居ること。**パースは `guff::parser` を使う**（`crates/guff-ast/src/parser.rs` のAPIを `Read` で確認）。
- **コミット**: `guff: port resolver.go object collection (chunk 22)`

#### [x] Step 23 — `decl.rs`（宣言の型チェック） — **done**: objDecl(簡易状態機械)/typeDecl(defined+alias)/collectMethods/funcDecl(sig half)/constDecl/varDecl(single-lhs)。assignment/initConst/initVar(check_assign.rs)。D18回収(Const/Var set_typ/set_val)。**残deferral**: 関数body(chunk30)、多重lhs:単一rhs var spread(initVars)、generics。
- **Goソース**: `decl.go`（`objDecl`, `constDecl`, `varDecl`, `typeDecl`, `funcDecl`, `declStmt`）
- **作るRustファイル**: `src/decl.rs`（新規）。大きいので **23a: const/var, 23b: type/func** に割る。
- **参考**: `typexpr.rs`（step 21）, `assignments.rs`
- **ロジック（日本語）**:
  - `Checker::obj_decl(&mut self, obj: ObjectId)`: 既に型付け済みならreturn。サイクル検出のため `push`/`pop`。種類で分岐して各 `*_decl` を呼ぶ。
  - `const_decl`: 型式があれば `typ`、初期化式を評価し `representable` で確定、Const.typ/val を設定。
  - `var_decl`: 型式 and/or 初期化式から型を決め（`assignment`）、Var.typ を設定。
  - `type_decl`: `new_named` で器を作り、`set_underlying(typ(rhs))`。エイリアスなら `new_alias`。型パラメータがあれば `bind_tparams`。
  - `func_decl`: シグネチャを `func_type`（step 24）で作り、本体は `later()` で遅延（本体検査は `stmt.rs` 完成後）。
- **deferral**: 関数本体の検査（stmt.rs待ち → `later` で積むが中身はDEFERREDでもよい）、メソッドの受信者解決の一部、iota継承の細部。
- **テスト**: `tests/decl.rs`。`type T int` 後に T がNamed/underlying int、`const c int = 5` で c.val==5、`var v = 3` で v.typ==int。
- **コミット**: `guff: port decl.go const/var/type/func decls (chunk 23)`

#### [x] Step 24 — `signature.rs` の Checker部分（`func_type`） — done: `signature_check.rs`に`Checker::func_type`/`collect_params`(variadic→[]T)/`collect_recv`。型パラメータ/関数スコープ+param宣言(D20)はdefer。
- **Goソース**: `signature.go`（`(*Checker).funcType`, 受信者・型パラメータ・可変長の処理）
- **触るRustファイル**: `src/signature.rs`（既存に追記）＋必要なら `src/check.rs`。**2ファイルまで。**
- **ロジック（日本語）**:
  - `Checker::func_type(&mut self, recv, tparams, ftyp) -> TypeId`: パラメータFieldListを走査して各Varを作り、`new_tuple`。結果も同様。可変長 `...T` の最後のパラメータをSlice化。受信者があればVar化。`new_signature_type` で組む。型パラメータは `bind_tparams`。
- **deferral**: 受信者の型パラメータ照合、可変長の型集合検証（chunk-2 deferral）。
- **テスト**: `tests/signature_check.rs`。`func(a int, b ...string) bool` をパース→func_type→params/variadic/results を確認。
- **コミット**: `guff: port Checker.funcType (signature.go) (chunk 24)`

---

### 🟦 Tier 4-E: 式と文（ここが本体・最大）

> ここから先は1ファイルが巨大（expr.go 1458行, stmt.go 842行, call.go 991行, builtins.go 1124行）。
> **必ず複数サブステップに割る。1サブステップ＝Goの数関数だけ。** 下の割り方に従う。

#### [~] Step 25 — `expr.rs`（式の検査）※5サブステップ — **25a/25b/25c/25d done**: skeleton+ident, basic_lit, unary, binary/comparison/shift(定数畳み込み), convert_untyped/implicit_type_and_value/representation(check_expr_const.rs)。**残**: 25e CompositeLit/FuncLit→literals.rs, Selector(26), Index/Slice(28), Call(27)。
- **Goソース**: `expr.go`
- **作るRustファイル**: `src/expr.rs`（新規・追記を繰り返す）
- **参考**: `operand.rs`, `conversions.rs`, `predicates.rs`, `under.rs`
- **サブステップ割り（1回1つ）**:
  - **25a**: `Checker::expr` / `rawExpr` / `exprInternal` の骨格（match分岐の枠だけ作り、各ケースは `// DEFERRED` で埋める）＋ `Ident` ケース（変数・定数・型名の解決）。
  - **25b**: `BasicLit`（リテラル）＋ `unary`（単項演算）＋定数畳み込みの数値部分。
  - **25c**: `binary`（二項演算）＋比較演算＋shift。`guff_constant` のBinaryOp/Compare/Shiftを使う。
  - **25d**: `convertUntyped` / `implicitTypeAndValue` / `updateExprType`（未型付き→確定）。step 20bの `representable` を使う。
  - **25e**: `CompositeLit`（複合リテラル）は `literals.go` 相当 → **別ファイル `literals.rs` に回してもよい**。ここはDEFERREDで飛ばして先に進んでよい。
- **各サブステップ共通ロジック（日本語）**:
  - `Checker::expr(x: &mut Operand, e: &Expr)`: 式 `e` を検査し、結果（mode/typ/val）を `x` に詰める。Goの `rawExpr→exprInternal` の流れと、`operand` の各modeへの設定を忠実に。
  - 演算は既存の `guff_constant` 関数に委譲。型の整合は `predicates::identical` と `assignable_to`。
- **deferral**: CompositeLit（→literals）、FuncLit（→literals）、Selector（→step 26と連携）、Index/Slice（→step 28）、Call（→step 27）。これらは expr のmatch内で各stepに対応する関数を呼ぶ形にし、未実装の間は `// DEFERRED` でInvalidを返す。
- **テスト**: `tests/expr.rs`。`1+2`→定数3:int、`true && false`→定数false:bool、`"a"+"b"`→定数"ab":string、変数参照→その変数の型。各サブステップごとにテストを足す。
- **コミット**: 各サブステップで `guff: port expr.go <部分> (chunk 25x)`

#### [x] Step 26 — `call.rs` の Selector — **done (2026-06-19)**: `Checker::selector(x, e, want_type)`。field選択(FieldVal→Variable/Value)、method value(MethodVal→recv除去Signature)、method expression(`T.M`→recvを第1paramに昇格)、NotFound/Ambiguous/PtrRecvRequired/builtin/wantType各エラー。`lookup_field_or_method`(既存)→objDecl(method)→Signature合成。expr_internalにSelectorExpr配線。tests/call.rs 4件。**Deferral**: pkg修飾子(`pkg.X`)=PkgName/importer無し(D16)、cgo特例、recordSelection/addDeclDep no-op、interfacePtrError/lookupErrorは簡易メッセージ。
- **Goソース**: `call.go`（`selector`, `(*Checker).callExpr` のうちselector部分）＋ `recording.go`（`recordSelection`）
- **作るRustファイル**: `src/call.rs`（新規、まずselectorだけ）。recording は no-op でも可。
- **ロジック（日本語）**:
  - `Checker::selector(x: &mut Operand, e: &SelectorExpr)`: `x.f`/`pkg.X` を解決。`lookup_field_or_method`（既存）→ `selection_type`（既存）でSelectionを作り、`x` の型を設定。
- **deferral**: パッケージセレクタ（import未実装）、recording全般。
- **テスト**: `tests/call_selector.rs`。structのフィールド選択、Named型のメソッド選択。
- **コミット**: `guff: port selector resolution (call.go) (chunk 26)`

#### [x] Step 27 — `call.rs` の呼び出し本体 — **done (2026-06-19)**: `Checker::call_expr(x, call)` — `fun`を`expr`で検査→mode別分岐。**conversion**(`T(x)`): 単一引数、`convertible_to`で判定、定数はそのまま保持(Checker.conversion畳み込みはD09)。**builtin**: chunk29待ちでinvalid(DEFERRED)。**通常呼出し**: underlyingがSignatureか確認(非関数→InvalidCall)、`arguments`(可変長対応の引数数チェック+各引数を`assignment`でparam型に照合)、結果は0→NoValue/1→Value+結果型/多→Value+tuple。`param_type`が可変長スプレッド(`...`/elem型)を計算。`use_args`。`expr_internal`にCallExpr配線。tests/call.rs +7件(simple/no-result/wrong-count/variadic×2/conversion/non-func)。**Deferral**: ジェネリック呼出し(infer plumbing=D21でinvalid化)、`f[int]()`明示インスタンス化、多値単一引数(genericExprList展開)、reverse type inference、hasCallOrRecv/record。
- **Goソース**: `call.go`（`callExpr`, `arguments`, `genericExpr`, 型変換としての呼び出し）
- **触るRustファイル**: `src/call.rs`（追記）
- **ロジック（日本語）**:
  - `Checker::call(x, e)`: `e.Fun` を検査→ mode別分岐（型変換 / builtin / 通常呼び出し）。
  - 通常呼び出し: 引数を `expr` で検査→ `arguments` でパラメータと突き合わせ（`assignable_to`）。ジェネリックなら `infer`（既存）で型引数推論。
- **deferral**: builtin（step 29）、可変長呼び出しの細部、reverse type inference。
- **テスト**: `tests/call.rs`。`f(1,2)`（`func f(int,int)int`）→ 結果int、引数不一致→エラー。
- **コミット**: `guff: port call expressions (call.go) (chunk 27)`

#### [x] Step 28 — `index.rs`（添字・スライス式） — **done (2026-06-19)**: `Checker::index_expr(x, e)`(string→byte値/array→elem(variable維持)/ *array→variable elem/slice→variable elem/map→key assignment+MapIndex)、`slice_expr`(string/array(unaddressableエラー)/ *array/slice、index順序チェック=SwappedSliceIndices)、`index`(定数out-of-bounds検出)、`is_valid_index`(convert_untyped→int+整数チェック+非負+int表現可能)。go/ast形(IndexExpr=単一index)なのでsingleIndex/ListExpr不要。`expr_internal`にIndexExpr/SliceExpr配線。`operand_str`をpub(crate)化。tests/index.rs 9件。**Deferral(D22)**: generic instantiation(`T[int]`/`f[int]`)、型パラメータoperand(underIs/Interfaceブランチ)、record。
- **Goソース**: `index.go`
- **作るRustファイル**: `src/index.rs`（新規）
- **ロジック（日本語）**:
  - `Checker::index_expr` / `sliceExpr`: 配列/スライス/マップ/文字列/ポインタ配列の添字、型パラメータの添字。結果mode（Variable/MapIndex/CommaOk）と要素型を設定。
- **deferral**: ジェネリック添字の一部、定数文字列添字の細部。
- **テスト**: `tests/index.rs`。`a[0]`（`[]int`）→int variable、`m["k"]`（map）→ value CommaOk、`s[1:2]`→slice。
- **コミット**: `guff: port index/slice expressions (index.go) (chunk 28)`

#### [x] Step 29 — `builtins.rs`（組み込み関数）※3サブステップ — **done (2026-06-19)**: `Checker::builtin(x, call, id)`。**29a**: 共通前処理(... 制限/引数評価/個数チェック)+append(custom variadic sig→arguments)/len・cap(string/array(定数)/slice/chan/map)/copy(elem identical)。**29b**: make(slice=2/map・chan=1引数, size index, swapped len/cap)/new(`Checker::typ`→`*T`)/delete(map key assignment→NoValue)/clear(map|slice→NoValue)。make/newは特殊扱いで通常引数評価をスキップ。**29c**: close(sendable chan)/complex・real・imag(typed+定数畳み込み, go/constant real/imag/to_float/make_imag)/min・max(ordered+matchTypes+定数compare)/panic・print(assignment)/recover(→any)。call_exprのbuiltinブランチ配線(chunk27 D21の一部回収)。tests/builtins.rs 26件。**Deferral**: 型パラメータoperand(underIs/applyTypeFunc)→underlying直、unsafe.*(D23, sizes.go必要)、append/copyのstring特例、hasCallOrRecv、new(expr)。
- **Goソース**: `builtins.go`
- **作るRustファイル**: `src/builtins.rs`（新規）
- **サブステップ割り**:
  - **29a**: `len`, `cap`, `append`, `copy`
  - **29b**: `make`, `new`, `delete`, `clear`
  - **29c**: `complex`, `real`, `imag`, `close`, `panic`, `recover`, `print`, `min`, `max`, `unsafe.*`
- **ロジック（日本語）**:
  - `Checker::builtin(x, call, id: BuiltinId)`: `id`（既存 `BuiltinId` enum）でmatch。各組み込みの引数検査と戻り型決定をGo通りに。`len`/`cap` は定数になる場合あり。
- **deferral**: `unsafe.Sizeof`/`Offsetof`/`Alignof`（sizes.go必要 → DEFERRED）。
- **テスト**: `tests/builtins.rs`。`len([]int)`→int、`make([]int, 3)`→[]int、`append(s, x)`→[]int など各サブステップで。
- **コミット**: 各サブステップで `guff: port builtin <群> (chunk 29x)`

#### [x] Step 30 — `stmt.rs`（文の検査）※5サブステップ — **done (2026-06-19)**
- **Goソース**: `go/types/stmt.go`（go/ast形）＋ `return.go`/`range.go`。
- **作ったRustファイル**: `src/stmt.rs`（新規）。assign helperは `check_assign.rs`、`decl_stmt`は`decl.rs`に追加。
- **完了したサブステップ**:
  - **30a-1**: `stmt`/`stmt_list`/`simple_stmt`骨格＋`open_scope`/`close_scope`＋`StmtContext`ビットセット＋`ExprStmt`（call以外の式=UnusedExpr/builtin=UncalledBuiltin/型=NotAnExpr）。
  - **30a-2**: `DeclStmt`（`decl_stmt`: ローカル const/var/type）＋`AssignStmt`（`=`/`:=`/複合代入）。`lhs_var`/`assign_var`/`assign_vars`/`init_vars`/`short_var_decl`を`check_assign.rs`に追加。n:1多値スプレッド(multiExpr)はdefer。
  - **30a-3**: `IncDecStmt`（`x<op>1`合成+NonNumericIncDec）＋`SendStmt`（`send_chan_elem`でsendable chan確認）。
  - **30b-1**: `BlockStmt`/`IfStmt`/`ForStmt`（`check_block`、`all_boolean`条件チェック、InvalidPostDecl）。
  - **30b-2**: `SwitchStmt`（tag代入+comparability、tagless=true、`multiple_defaults`、`case_values`=convert_untyped+comparison）。重複case値検出はdefer。
  - **30c-1**: `ReturnStmt`（`sig_results`と`init_vars(is_return)`照合）。named-result implicit returnのshadowチェック/returnErrorはdefer。
  - **30c-2**: `RangeStmt`＋`range_key_val`（string/array/*array/slice/map/chan/integer）。`:=`/通常代入両対応。func iterator(go1.23)/型集合commonUnderはdefer。
  - **30d**: `BranchStmt`（break/continue/fallthrough配置チェック）＋`GoStmt`/`DeferStmt`（`suspended_call`=call引数検査; conversion/discards分類はdefer）＋`SelectStmt`（comm clause検証）。
  - **30e（マイルストーン）**: **func body配線**。`func_decl`が`check.later`で`func_body`を遅延予約→`func_body`が関数スコープを開いてrecv/params/named-resultsを宣言し`env.sig`設定して`stmt_list`で本体検査。**check_filesが関数本体を完走**（return型・ローカル宣言・制御フロー）。
- **未対応(D24)**: ~~`TypeSwitchStmt`~~（**chunk 34で回収**）、`labels.go`2nd pass（ラベル解決/hasLabel）、`isTerminating`(MissingReturn)、`usage`(declared-and-not-used)、reachability、go/defer parse quirk（既存parserの`exprs_eq_ptr`がclone比較で常にdiff→`defer f()`がparenthesized扱い）。
- **テスト**: `tests/stmt.rs`（30件）＋`tests/check_files.rs`（+3: body検査/return型エラー/forward ref）。**411 tests pass**。

#### [x] Step 31 — `literals.rs`（複合リテラル・関数リテラル）— **done (2026-06-19)**
- **Goソース**: `go/types/literals.go`。**作ったRustファイル**: `src/literals.rs`。
- **31a `func_lit`**: `func_type`(chunk24)でSignature構築(typexprのfunc型は未対応なので直接)、本体は`self.later(|c| c.func_body(sig,parent,&body))`(chunk30e再利用)。`expr_internal`にFuncLit配線。check_filesが関数リテラル本体を完走。
- **31b `composite_lit(x, e, hint)`**: 型をe.Type(`self.typ`)かhint(`*T`は`common_under`+`deref`)で決定。`common_under`のunderlyingで分岐: **Struct**(`composite_struct`: keyed=全要素key必須/field_index/重複検出, positional=順序/unexported field/個数チェック)、**Array/Slice**(`indexed_elts`: index検証/out-of-bounds=OversizeArrayLit/重複index=DuplicateLitKey)、**`[...]T`**(elem算出→indexed_elts(len=-1)で個数→`new_array`)、**Map**(`composite_map`: key/value各assignment)。`expr_internal`にCompositeLit配線。
- **テスト**: `tests/literals.rs` 10件(slice/array/[...]/map/keyed slice/oversize/dup index/missing map key/wrong elem type/nested explicit)。**423 tests pass**。
- **Deferral(D25)**: `exprWithHint`未配線→typeless nested literal(`[][]int{{1}}`内側)はUntypedLitエラー(明示型が要る)。map重複key検出(keyVal/visited)、struct literalのdriver経由テスト(typexprのstruct型式未対応=chunk21 defer)、Info recording。

#### [x] chunk 34 — 型アサーション `x.(T)` と型switch（D24一部回収）— **done (2026-06-19)**
- **Goソース**: `expr.go`(`typeAssertion`, AssertExpr case)、`lookup.go`(`assertableTo`/`hasAllMethods`)、`stmt.go`(`typeSwitchStmt`/`caseTypes`/`isNil`)。
- **chunk34a `expr.rs`/`check_lookup.rs`**: `Checker::has_all_methods`/`assertable_to`/`type_assertion`を`check_lookup.rs`に追加(chunk11 deferral=assertableToを回収)。`expr_internal`に`TypeAssertExpr`(`x.(T)`)を配線→`type_assert`(operand非interface=InvalidAssert/型パラメータ=InvalidAssert/`.(type)`単独=InvalidSyntaxTree/不可能=ImpossibleAssert; mode=CommaOk)。`tests/type_assert.rs` 4件。
- **chunk34b `stmt.rs`**: `TypeSwitchStmt`の dispatch arm。guard(`[v :=] x.(type)`)を ExprStmt/AssignStmt から抽出→operandがinterfaceか確認(非interface/型パラメータ=InvalidTypeSwitch)→`case_types`(各case: `is_nil_expr`でnil case、`self.typ`で型解決、Identicalで重複検出=DuplicateCase、type-switch modeで`type_assertion`=ImpossibleAssert)→case-localの束縛変数(`v`)を宣言。`tests/type_switch.rs` 6件。**446 tests pass**(+10)。
- **Deferral(D24残)**: `labels.go`2nd pass、`isTerminating`、`usage`(declared-and-not-used; type switchの束縛変数のcross-clause使用チェックも含む)、n:1多値代入スプレッド、return mismatch/named-result shadow、switch重複case**値**検出(goVal)、range func iterator、go/defer conversion分類、record系。

#### [x] chunk 35a — 型インスタンス化 `T[A]` / `T[A,B]`（D22一部回収）— **done (2026-06-19)**
- **Goソース**: `typexpr.go`(`typInternal` の `IndexExpr` case、`instantiatedType`、`genericType`、`typeList`)、`instantiate.go`(`validateTArgLen`、`instance`)。
- **`predicates.rs`**: `is_generic(arena, t)`(Alias=tparams有りtargs無し / Named=非instance かつ tparams>0)を追加・export。
- **`typexpr.rs`**: `typ_internal` に `IndexExpr`(単一引数)/`IndexListExpr`(複数引数)を配線→`instantiated_type(x, xlist, pos)`。`generic_type`(typ_internal+`is_generic`チェック、非generic=NotAGenericType)、`type_list`(各引数を`typ`、1つでもinvalidなら`None`)、`validate_targ_len`(WrongTypeArgCount)を追加。インスタンスは既存の`instantiate::instantiate`(Context dedup + 構造的展開)で生成。
- **テスト**: `tests/instantiation.rs` 5件。生成的型**宣言**(`type Box[T any]`)はdecl.rs未対応(D19)なので、generic Namedをarenaに直接構築しuniverse scopeにinsertして`Vec[int]`をドライブ(chunk26 selector testの前例踏襲)。single-arg/dedup/NotAGenericType/WrongTypeArgCount/invalid-targ。**451 tests pass**(+5)。
- **Deferral(D27)**: `Checker.verify`(型引数が型パラメータの制約boundを満たすか=`implements(targ, bound, constraint=true)`の`satisfies`ロジック)未実装→引数**個数**のみ検証。`recordInstance`/`mono.recordInstance` no-op。関数値インスタンス化`f[int]`(index.rs/call.rs, D21/D22残)も未。

#### [x] chunk 35a-decl — generic型宣言 `type T[P ...] U`（D19一部回収）— **done (2026-06-19)**
- **Goソース**: `decl.go`(`typeDecl`の型パラメータ分岐、`collectTypeParams`、`bound`、`declareTypeParam`)。
- **`decl.rs::type_decl`**: Named生成後、`tdecl.type_params`が非空なら`open_scope`("type parameters")→`collect_type_params(named, fl)`→RHS型検査→`close_scope`。
- **`collect_type_params`**: go/astのグループ名(`[A, B any]`=1 Field 2 names)をフラット化。各nameを`declare_type_param`(=`new_type_name`+`new_type_param`(placeholder=Invalid bound)+`type_name_set_typ`副作用+scope `declare`)で宣言→`bind_tparams`→`named_set_type_params`(bound解決**前**にセット, go.dev/issue/47887)→各Fieldの`bound`を解決し`set_constraint`(グループ名は前のboundを再利用)。型パラメータをconstraintにした場合MisplacedTypeParam。
- **`bound`**: `~T`/`A|B`(UnaryExpr TILDE / BinaryExpr OR)は暗黙interfaceに包んで`interface_type`へ、それ以外は`self.typ`。
- **テスト**: `tests/instantiation.rs` source群4件(`Vec[T any]`宣言+`Vec[int]`/`Pair[K comparable, V any]`+`Pair[int,string]`/グループ名`[A,B any]`/個数誤りWrongTypeArgCount)。**455 tests pass**(+4)。**`T[int]`のsource経由end-to-endが通るようになった**(chunk35aの前提解消)。
- **Deferral**: generic **alias**の型パラメータ(`type A[P any] = ...`)未対応、TypeName位置情報(D07: set_pos accessor無し)。

#### [x] chunk 35b — 制約満足検査 `Checker.verify`（D27回収）— **done (2026-06-19)**
- **Goソース**: `instantiate.go`(`verify`)。`implements`(constraint=true パス)は既にchunk20a/34で移植済(`check_lookup.rs`がverb="satisfy"・comparability spec/dynamic・typeset subset/inclusion全対応)。
- **`typexpr.rs`**: `verify_targs(tparams, targs)`を追加(`make_subst_map`→各tparamのboundを`subst`で型引数置換→`implements(targ, bound, constraint=true)`; 最初の違反index+causeを返す)。`instantiated_type`が`validate_targ_len`成功後に`verify_targs`を呼び、違反は`InvalidTypeArg`の**ソフトエラー**(Goの`softErrorf`同様、報告してもインスタンスは生成・返却)。エラー位置は違反した`xlist[i]`。
- **テスト**: `tests/instantiation.rs` +5(comparable満足/違反`Set[[]int]`、union制約`Ordered interface{~int|~string}`満足`Box[int]`/違反`Box[bool]`、インライン`[T ~int|~string]`違反)。**460 tests pass**(+5)。
- **Deferral**: `mono.recordInstance`/`recordInstance` no-op、go1.20 comparability version gate(満足扱い)、`tpar.iface()`の明示呼出し省略(boundは既にinterface)。

#### [x] chunk 35c — generic関数宣言 `func F[T any](...)`（D20一部回収）— **done (2026-06-19)**
- **Goソース**: `signature.go`(`funcType`: openScope→collectRecv→collectTypeParams→collectParams)、`decl.go`(`collectTypeParams`)。
- **`decl.rs`リファクタ**: `collect_type_params`を`declare_type_params`(各nameを現スコープに宣言→(tparam ids, field-of)返却)と`resolve_type_param_bounds`(bound解決+set_constraint)に分割し`pub(crate)`化(`bound`/`declare_type_param`も)。
- **`signature_check.rs::func_type`**: 関数スコープを`open_scope`("function")で開き(D20: sig.scopeフィールドは無いので一時的)、recv収集→`ftyp.type_params`があれば`declare_type_params`+`bind_tparams`+`resolve_type_param_bounds`→`collect_params`(型パラメータがスコープに見える状態でparams/results解決)→`close_scope`→`signature_set_type_params`。method+型パラメータは`InvalidMethodTypeParams`。
- **`stmt.rs::func_body`**: 関数body scopeに型パラメータのTypeNameも宣言(`signature_type_param_objs`)→body内で`T`が解決可能。
- **`signature.rs`**: `signature_type_params`free accessor追加・export。
- **テスト**: signature_check.rs +3(`F[T any](x T) T`の型パラメータ収集+param/result=T/`Map[T,U any]`複数/package scopeに漏れない)、check_files.rs +1(`Id[T any](x T) T { var y T = x; return y }`本体が`T`参照しエラー0)。**464 tests pass**(+4)。
- **Deferral**: 受信者型パラメータ(`func (r T[P]) M()`の`rparams`/`unpackRecv`)、generic method、generic func呼出しの型推論(D21)、sig.scopeフィールド(body は別scopeで再宣言)。

#### [x] chunk 35d — generic呼出しの型推論 `f(args)`（D21回収）— **done (2026-06-19)**
- **Goソース**: `call.go`(`callExpr`の通常呼出し分岐、`arguments`の型推論スライス)、`infer.go`(`infer`, chunk13で移植済)。
- **`call.rs::call_expr`**: generic callee の bail-out(D21)を撤去。`arguments`が`bool`→`Option<TypeId>`(推論後の具体sig)を返すよう変更し、resultはその具体sigから読む。
- **`call.rs::arguments`**: callee の型パラメータ(sig.tparams)があれば`infer_call`を呼び、推論成功なら`instantiate`で具体sig生成。引数チェックはその具体sigのparamに対して`assignment`。
- **`infer_call`**: `call_param_tuple`(可変長tailを引数数まで展開=`infer`が`params.len()==args.len()`を要求するため)→引数型ベクタ(invalid operand=None)→`infer(tparams, [None;n], params, args, false)`。`InferResult::Ok`→`instantiate(sig, targs)`、`Failed`→`CannotInferTypeArgs`報告+None。
- **テスト**: call.rs +5(`Id(a:int)`→int/`Pair(i,s)`→string/可変長`First(a,b)`→int/制約外`Zero()`=CannotInfer/競合`Eq(i,s)`=CannotInfer)、check_files.rs +2(`var r int = Id(a)`完走/`var r string = Id(a)`型不一致エラー)。**471 tests pass**(+7)。
- **Deferral**: 明示/部分型引数`f[int](x)`(IndexExpr fun)、`renameTParams`(再帰呼出し)、reverse type inference(generic関数引数)、未型付き引数のdefault-type昇格(infer step3=D11)。型側genericsに続き**関数側genericsの宣言+呼出し推論まで完走**。

#### [x] chunk 35e — generic method受信者 `func (r T[P]) M()`（D20残回収）— **done (2026-06-19)**
- **Goソース**: `signature.go`(`collectRecv`)、`resolver.go`(`unpackRecv`)。
- **`signature_check.rs::collect_recv`**: 戻り値を`(Option<ObjectId>, Option<TypeParamList>)`に変更。`unpack_recv`で受信者型`[*]B[P...]`を(ptr, base, tparam names)に分解(go/ast: `*B`=StarExpr/`B[P]`=IndexExpr/`B[P,Q]`=IndexListExpr; 非ident param=BadDecl→`_`)。型パラメータ有り: `generic_type(base)`でNamed解決(Alias=不可)→受信者型パラメータを関数scopeに`declare_type_param`→`bind_tparams`→base型パラメータのboundを`subst`(base→recv rename map)でコピー→`instantiate(base, recv_tparams)`で受信者型生成(ptrなら`new_pointer`)。arity不一致=BadRecv。`func_type`が`signature_set_recv_type_params`。
- **`signature.rs`**: `signature_recv_type_params` free accessor追加・export。
- **テスト**: signature_check.rs +3(`Box[T]`受信者の型パラメータ収集+受信者型=Named+result=T/ポインタ受信者`*Box[T]`/arity不一致=BadRecv)、check_files.rs +1(`func (b Box[T]) Get() T { return b.v }`完走)。**475 tests pass**(+4)。**genericsは型宣言/インスタンス化/制約/関数宣言/メソッド/呼出し推論まで一通り完走**。
- **Deferral**: `validRecv`(later)、generic alias method=エラー化省略、`mono.recordCanon`、Info recording。

#### [x] chunk 36 — declared and not used（`usage`, D24一部回収）— **done (2026-06-19)**
- **Goソース**: `stmt.go`(`usage`、`ident`の`usedVars`マーク、`funcBody`の`usage(sig.scope)`呼出し)。
- **`check.rs`**: `used_vars: HashSet<ObjectId>`フィールド追加。
- **`expr.rs::ident`**: Var解決時、`obj.pkg()==self.pkg`なら`used_vars`に挿入(他パッケージ変数は無視=dot-import競合回避)。
- **`stmt.rs`**: `func_body`が関数scopeを`set_is_func(true)`にし、body検査後に`usage(fscope)`を呼ぶ。`usage(scope)`: scope内のlocal Var(kind≠Recv/Param/Result)で`used_vars`に無いもの→`UnusedVar`(pos順)。非func子scopeに再帰(func literal scopeはそれ自身のfunc_bodyで処理されるのでスキップ)。型switch束縛変数は`used_vars`に事前挿入してexempt(cross-clause検査はD24残)。
- **テスト**: check_files.rs +6(未使用local=error/使用=ok/`x:=1`未使用=error/param・result除外/ネストblock未使用/`_=x`は使用扱い)。**481 tests pass**(+6)。
- **Deferral(D24残)**: 型switch束縛変数のcross-clause使用検査、`:=`再宣言(no new vars)検査、`labels.go`2nd pass、`isTerminating`(MissingReturn)。

#### [x] chunk 37 — missing return（`isTerminating`, D24一部回収）— **done (2026-06-19)**
- **Goソース**: `return.go`(`isTerminating`/`isTerminatingList`/`isTerminatingSwitch`/`hasBreak`系)、`stmt.go`(`funcBody`末尾のMissingReturn)。
- **`return_check.rs`新規**: AST上の純構造述語(free fn)。`is_terminating(s, label)`(Return/panic呼出しExprStmt/goto・fallthrough/Block/If-else両分岐/Switch・TypeSwitch=`is_terminating_switch`/Select/`for{}`無cond無break)、`is_terminating_list`(末尾の非空文)、`has_break`系(ラベル付きbreak照合; go/astのSwitch/TypeSwitch/Select/For/Rangeを分けて処理)。panicは`unparen(call.fun)`がIdent"panic"の構造判定(shadowing非対応の簡略化)。
- **`stmt.rs::func_body`**: usage後、`sig.results`>0かつ`!is_terminating_list(&body.list, "")`なら`body.end()`(Rbrace)に`MissingReturn`。
- **テスト**: check_files.rs +8(空body=error/trailing return=ok/結果無=ok/if-else両return=ok/if単独=error/panic=ok/`for{}`=ok/`for{break}`=error)。**489 tests pass**(+8)。
- **Deferral(D24残)**: 型switch束縛のcross-clause使用、`labels.go`2nd pass、`:=`再宣言、record系。

#### [x] chunk 38 — invalid recursive type（`validType`配線, Tier5 Step33一部）— **done (2026-06-19)**
- **Goソース**: `decl.go`(`typeDecl`末尾の`check.later(validType)`)、`validtype.go`(chunk10で`validtype.rs`移植済)。
- **`decl.rs::type_decl`**: defined type の underlying設定後、`self.later(|c| ...)`で`valid_type(named)`を予約。`ValidResult::Cycle{path}`なら`InvalidDeclCycle`("invalid recursive type: T refers to itself" / "invalid recursive type T")を`obj.pos()`に報告。`valid_type`はサイクル検出時に内部で`Named::invalidate`も行う。
- **テスト**: check_files.rs +4(`type T struct{x T}`=error/`*T`ポインタ経由=ok/`[]T`スライス経由=ok/相互再帰`A{b B},B{a A}`=error)。**493 tests pass**(+4)。
- **Deferral**: Goの`cycleError`の多行詳細メッセージ("cycle[i] refers to cycle[j]")は簡略化。alias経由サイクル(`type A = [10]A`)は別途。`mono`系。

#### [x] chunk 39 — label checking（`labels.go` 2nd pass, D24一部回収）— **done (2026-06-19)**
- **Goソース**: `labels.go`(`labels`/`blockBranches`/`block`)、`stmt.go`(`funcBody`の`check.labels(body)`)。
- **`labels.rs`新規**: Labelオブジェクト無しで名前ベース構造パス。`Checker::labels(body)`: pass1=全LabeledStmt収集(関数スコープ; DuplicateLabel)、pass2=`label_branches`再帰walk(enclosing=ラベル付きbreakable文スタック; 各labeled branch検証: break=任意enclosing/continue=Loopのみ/goto=`all`に存在; MisplacedLabel/UndeclaredLabel; 使用記録)、pass3=未使用→UnusedLabel(pos順)。`breakable_kind`(For/Range=Loop, Switch/TypeSwitch/Select=SwitchSelect)。go/astのSwitch/TypeSwitch/Select/For/Range/clause本体を辿る。
- **`stmt.rs::func_body`**: usage前に`self.labels(body)`。labelが無くても`goto L`(未宣言)検出のため常時実行。
- **テスト**: check_files.rs +5(labeled break=ok/未使用label=UnusedLabel/未宣言goto=UndeclaredLabel/重複=DuplicateLabel/非breakableへのbreak=MisplacedLabel)。**498 tests pass**(+5)。
- **Deferral**: forward jump解析(`JumpOverDecl`/`JumpIntoBlock`=var宣言/block跨ぎgoto)、`recordDef`/`recordUse`、Goの`hasLabel`最適化(常時walk)。

#### [x] chunk 40 — n:1多値代入 `a, b := f()` + channel receive（D24一部回収）— **done (2026-06-19)**
- **Goソース**: `assignments.go`(`initVars`/`assignVars`/`unpackExpr`/`multiExpr`)、`expr.go`(unary `<-` recv)。
- **`check_assign.rs`**: `eval_multi(e)`追加(単一式を評価し、型がTupleなら要素ごとにValue operandへ展開、それ以外は単一operand)。`init_vars`/`assign_vars`を「r==1 && l!=1 → `eval_multi`展開→個数一致なら各lhsへ/不一致=WrongAssignCount」「l==r → 1:1」「他=mismatch」に再構成。**旧`is_call`特例(call結果を捨てinitせずplaceholderのまま残すバグ)を解消** → `ch := make(chan int)`等のcall RHSが正しく型付く。
- **`expr.rs` unary `<-ch`**: chanのelem型を返す(非chan/send-only=InvalidReceive; mode=Value、2値comma-ok `v,ok:=<-ch`は未対応)。
- **テスト**: check_files.rs +5(`a,b:=two()`/`a,b=two()`/個数不一致=WrongAssignCount/型不一致/`v:=<-ch`)。**503 tests pass**(+5)。
- **Deferral**: comma-ok 2値展開(`v,ok:=m[k]`/type assert/recv)、`var a,b=f()`のpackageレベル(decl.rs var_decl単一lhsのまま)。

#### [x] chunk 41 — comma-ok 2値展開 `v, ok := m[k]` / `x.(T)`（D24一部回収）— **done (2026-06-19)**
- **Goソース**: `assignments.go`(`multiExpr`の`allowCommaOk`)。
- **`check_assign.rs::eval_multi`**: `want`引数追加。`want==2`かつoperand modeが`MapIndex`(map index)/`CommaOk`(type assertion)なら`(value, bool)`の2 operandへ展開(value=operand型, ok=`bool`)。`init_vars`/`assign_vars`が`eval_multi(rhs[0], l)`を呼ぶ。単一値パスは不変(回帰なし)。
- **テスト**: check_files.rs +3(`v,ok:=m[k]`/`v,ok:=i.(int)`/`ok`がboolでintでない検証)。**506 tests pass**(+3)。
- **Deferral**: channel receive comma-ok `v,ok:=<-ch`(recv operandは現在Value; CommaOk化が必要)→**chunk42で回収**。

#### [x] chunk 42 — channel receive comma-ok `v, ok := <-ch`（D24 comma-ok完了）— **done (2026-06-19)**
- **Goソース**: `expr.go`(unary `syntax.Recv` case: `x.mode = commaok; x.typ = elem`)。
- **`expr.rs` unary `<-ch`**: 受信結果のmodeを`Value`→`OperandMode::CommaOk`に変更(Go通り)。`x.typ`はchan elem型のまま。単一値コンテキストではassignment/eval_multiがCommaOkを通常Valueとして扱うので不変、2値コンテキストでは既存の`eval_multi`(chunk41)がmap index/type assertと同じく`(elem, bool)`へ展開。**変更は実質1行**(コメント更新含む)。
- **テスト**: check_files.rs +2(`v,ok:=<-ch`→v=int/ok=bool完走、`ok`をintに代入=型エラー)。既存`channel_receive_value`(単一値`v:=<-ch`)も回帰なし。**508 tests pass**(+2)。
- **Deferral**: `hasCallOrRecv`追跡(Goは`check.hasCallOrRecv = true`; recordに使うがInfo未配線=D07/D14)。これでcomma-ok 3形態(map index/type assert/recv)が全て揃いD24のcomma-ok項目は完了。

#### [x] chunk 43 — switch 重複 case **値**検出 `caseValues`/`goVal`（D24一部回収）— **done (2026-06-19)**
- **Goソース**: `stmt.go`(`caseValues`の`seen valueMap`+`goVal`)。gc互換でinteger/float/string定数のみ重複検査。
- **`stmt.rs`**: module level に `CaseKey`(Int(i64)/Uint(u64)/Float(u64 bits)/Str(String); `#[derive(Hash,Eq)]`)と`go_val(&Value)->Option<CaseKey>`(Goの`goVal`: Int→int64/uint64フォールバック、Float→float64 bits、String→string、他=None)を追加。`case_values`に`seen: &mut HashMap<CaseKey, Vec<(TypeId, u32)>>`引数を追加し、`comparison`後 `res.mode`有効&&`v.mode==Constant`なら`go_val`でkey化→既存typeを`identical`比較(同値でも型が違えば別case: `byte(1)`vs`myByte(1)`)→重複なら`DuplicateCase`、無ければ`seen`へpush。expr switch driverが`seen`をclause loop前に1個生成しthread。
- **テスト**: check_files.rs +5(`case 1: case 1:`=dup/`case 2, 2:`同clause内dup/string dup/distinct=ok/非定数`case y: case y:`=ok)。**513 tests pass**(+5)。
- **Deferral**: Goの2エラー(duplicate + "previous case")は単一`DuplicateCase`に簡略(errors.rsが単一メッセージ=chunk19方針)。switch tag が型パラメータの場合の型集合越し比較は未。

#### [x] chunk 44 — map リテラル重複キー検出 `keyVal`/`visited`（D25一部回収）— **done (2026-06-22)**
- **Goソース**: `literals.go`(`*Map` case の`visited valueMap`+`keyVal`)、`expr.go`(`keyVal`)。
- **`literals.rs`**: module level に `MapKey`(Int/Uint/Float bits/Complex(re,im bits)/Str/Bool; Hash+Eq)と`key_val(&Value)->Option<MapKey>`(Goの`keyVal`: complex→float→int正規化で`1`/`1.0`/`1.0+0i`が同一キー、bool/string含む、Unknownのみ None)。`composite_map`が`key_is_interface`(=`is_non_type_param_interface(key_t)`)で分岐: interfaceなら同一keyでも`identical`で型比較(`byte(1)`vs`myByte(1)`は別)、concreteなら値のみで重複判定。定数キーのみ検査、重複は`DuplicateLitKey`報告し`continue`(value式はskip、Go通り)。
- **テスト**: literals.rs +5(string dup/int dup/`1`vs`1.0`衝突/distinct=ok/非定数キー=ok)。**518 tests pass**(+5)。
- **Deferral(D25残)**: `exprWithHint`未配線(typeless nested literal)、Info recording。chunk43の`go_val`(switch)と`key_val`(map)は別関数(map側はbool/complex対応+float→int正規化で広い)。

---

### 🟦 Tier 4-F: ドライバを繋ぐ

#### [x] Step 32 — `check.rs` の `check_files` ドライバ — **done（マイルストーン: 型チェッカ完走）**: `Checker::check_files`(collect_objects→sort_objects→package_objects→process_delayed→complete)。`package_objects`(resolver.rs, 3-phase obj_decl)。小さなpackage(type/const/var/func+method+forward ref)がエラー0で通り、int8 overflowは検出。D17回収。残defer: initFiles version, directCycles, cleanup, initOrder, unusedImports, recordUntyped, monomorph。
- **Goソース**: `check.go`（`checkFiles`, `packageObjects`, `processDelayed`）
- **触るRustファイル**: `src/check.rs`（追記）＋ `src/resolver.rs`（連携）
- **ロジック（日本語）**: `Checker::check_files(files)`: `collect_objects` → `package_objects`（各obj_mapを `obj_decl`）→ `process_delayed`（関数本体）→ エラー収集。Goの `checkFiles` の呼び出し順をそのまま。
- **deferral**: `initOrder`（step 34）, `unusedImports`, `monomorph`（mono.go）, `cleanup`。
- **テスト**: `tests/check_files.rs`。**小さい完全なGoソース**（`package p; var x int = 1; func f() int { return x }`）をパース→check_files→エラー0、x/fの型が正しい。**これが「型チェッカが動いた」最初の証拠。重要なマイルストーン。**
- **コミット**: `guff: wire check_files driver (chunk 32)`

---

### 🟦 Tier 5: 仕上げ（順不同・優先度低め。check_filesが動いてから）

各1ステップ＝1ファイル。Goソースを読み、§4手順で淡々と移植。
- [~] Step 33 — `cycles.rs`（`cycles.go`: 型サイクル検出）。**chunk38で`validType`配線(type宣言の構造的サイクル検出)完了**。**chunk60で`directCycles`/`directCycle`移植**: パッケージレベル型宣言の**name-chain直接サイクル**(`type A B; type B A`/`type A = B; type B = A`/`type A A`=名前→名前で型リテラル無しで戻る)を検出。白灰黒(`path_idx: HashMap<ObjectId,i64>`)アルゴリズムで`obj_map[tname].tdecl.ty`がIdentならpkg scopeで次のTypeNameへ。サイクル始点を`Typ[Invalid]`化(obj_declがblack扱いでskip)+簡略`cycle_error`(単一`InvalidDeclCycle`、`first_in_src`はD07で実質index 0)。`check_files`の`sort_objects`と`package_objects`の間に配線。tests/cycles.rs 7件。**650 tests pass**(+7)。残: `finiteSize`(Named finite-size状態機械`hasFinite`/`finite`+`objPathIdx`要)、`cycleError`の多行"X refers to Y"詳細(per-object pos=D07)、alias `fromRHS`/`validAlias`リセット(直接`Typ[Invalid]`化で代用)。
- [ ] Step 34 — `initorder.rs`（`initorder.go`: パッケージ初期化順）
- [x] Step 35 — `mono.rs`（`mono.go`: 単相化可能性）— **done (2026-06-25, chunk 58)**: `MonoGraph{vertices,edges,canon,name_idx}`(Checkerの`mono`フィールド)。型フロー有向重み付きグラフ: `record_instance`→`assign`→`do_walk`(型引数を再帰walk: TypeParam/Named(origin localNamedVertex+type args)/Array/Chan/Map/Pointer/Slice/Interface(tset methods)/Signature(params+results)/Struct)、`flow`(typ==targ?0:1 weight)、`type_param_vertex`(canon解決+頂点生成)、`local_named_vertex`(scope chain walkでambient型パラメータ検出)、`add_edge`。`Checker::monomorph`(Bellman-Ford変種=最大重みパス、path長==|V|でサイクル)+`report_instance_loop`(InvalidInstanceCycle、多行二次エラーは単一メッセージに簡略)。配線: typexpr `instantiated_type`(verify成功時)+call `infer_call`(推論成功時)で`mono.record_instance`、check_files末尾(first_err無時)で`monomorph`。tests/mono.rs 8件(MonoGraph直接駆動: pointer/map self=cycle, identity/swap=zero-weight非cycle, concrete無edge, foreign pkg無視 + 全package smoke 2件)。**634 tests pass**(+8)。**Deferral**: (1)source経由のサイクル検出はinfer(D11: `T:=*T`等パラメータ化推論結果を拒否)+明示`f[T]()`(D21)未対応でブロック→graph直接テスト。(2)`record_canon`配線(generic method receiver site)未=method型パラメータは独自頂点(保守的=サイクル見逃しはあれど誤検出無)。(3)`local_named_vertex`のpos gate(`elem.pos<obj.pos`)はD07(pos≈0)で実質no-op→ローカル定義型edge classは縮退(見逃しのみ、誤検出無)。(4)`MonoEdge.pos`は二次エラー位置用に保持(現状未使用)。
- [x] Step 36 — `sizes.rs`（`sizes.go`+`gcsizes.go`+`gccgosizes.go`: Sizeof/Alignof/Offsetsof）— **done (2026-06-23, chunk 45)**: `Sizes{kind:SizesKind(Std/Gc), word_size, max_align}` 単一構造体で `StdSizes`(gccgo)と`gcSizes`(gc)を表現(Sizeofの Array/Struct分岐のみ差異)。`alignof`/`offsetsof`/`sizeof`(ta/oa/pa引数, 純構造的, Checker非依存)、`align`(power-of-2)、`is_sync_atomic_align64`(named obj name/pkg path)、`sizes_for(compiler,arch)`+`default_sizes()`(=gc/amd64 {8,8})、gc/gccgo arch表全移植、`basic_size`(basicSizes表)。tests/sizes.rs 12件。**530 tests pass**(+12)。**残**: builtin の `unsafe.Sizeof`/`Alignof`/`Offsetof` 配線(D23)はimports(D16)前提でまだ→`Config.Sizes`/`conf.alignof`/`offsetof`ドライバ未配線。
- [ ] Step 37 — `recording.rs` 本実装（`recording.go`: Info記録）→ step 18b のInfo no-op を回収（ASTノードIDが要るので18bの判断次第）
- [x] Step 38 — `api_predicates.rs`（`api_predicates.go`: 公開述語ラッパ）— **done (2026-06-25, chunk 54)**: `AssignableTo`/`ConvertibleTo`/`Implements`/`Satisfies`/`AssertableTo`を arenaベースfree fn `api_*`で移植(Goの`(*Checker)(nil)`→3 arena明示引数)。`api_identical`/`api_identical_ignore_tags`は`predicates::identical`の別名。既存の operand ベース述語+`check_lookup::implements`/`missing_method`の薄いラッパ。tests/api_predicates.rs 18件。**605 tests pass**。
- [~] Step 39 — `util.rs` / `version.rs`（`util.go`, `version.go` の小物）。**`version.rs` done (2026-06-25, chunk 55)**: `GoVersion`/`as_go_version`/言語変更バージョン定数(go1_9..go1_26, go_current)/`Checker::allow_version`/`verify_versionf`/`version_errorf`(UnsupportedFeature)。`env.version`を読む(空=全許可)。tests/version.rs 8件。**613 tests pass**。残: `util.rs`(`util.go`小物)、実バージョンゲート配線。
- [x] Step 40 — `format.rs`（`format.go`: メッセージ整形）— **done (2026-06-25, chunk 59)**: Goの`sprintf(qf,tpSubscripts,...)`は`fmt.Sprintf`引数事前レンダラ。Rustは可変長printf無=呼出し側が`format!`+`type_str`で組むので、`sprintf`自体は移植せず**各引数レンダラ**を移植: `strip_annotations`(添字数字₀..₉=U+2080..U+2089のみ除去、`#`/通常文字は保持=Goの`r<'₀'||'₀'+10<=r`ガード忠実)、`ndigits`(10進桁数、3上限の純ヘルパ)、free `qualifier(cur,pkg,parena)`+`Checker::qualifier(pkg)`(現パッケージ=""/他=pkg名、`(*Checker).qualifier`相当)、`Checker::type_list_str`(`[]Type`=`[T1, T2]`、qf配線)、`Checker::operand_list_str`(`[]*operand`=`[op1, op2]`)。**`errors.rs::type_str`を修飾子経由に更新**(旧:qf=None=全パッケージにフルpath接頭辞→新:現パッケージbare/他パッケージはname修飾、`unsafe`はtypestring側で特例処理済)。lib.rsで`ndigits`/`qualifier`/`strip_annotations` re-export。tests/format.rs 9件(strip_annotations/ndigits/qualifier×2/type_list_str/operand_list_str×2/type_str)。**643 tests pass**(+9)。**Deferral(D29)**: `qualifier`の`pkgPathMap`/`markImports`重複解決(同名2パッケージ→フルpath引用)はimporter(D16)待ち=現状bare name。`trace`/`dump`(stdoutデバッグ)は実pos(D07)+`check.indent`要で省略。`tpSubscripts`はtypestringが既に添字drop済で対応物無し。
- [ ] Step 41 — 残りdeferralの一括回収（§8の表を上から潰す）

---

## 8. deferral（後回し）追跡表

> **新しく後回しにしたら、ここに1行足す。回収したら「状態」を「回収済み」にする。**
> 移植開始時点（chunk 17完了時）の既知deferralを載せてある。

| ID | 場所(ファイル) | 内容 | 必要になる前提 | 状態 |
|----|------|------|------|------|
| D01 | termlist.rs | `equal` がchunk-4 `identical_stub` を使用（無名union dedupが不正確） | termlistに `&mut TypeArena` を通す | 未 |
| D02 | predicates.rs | `comparable_type` が `intersect_term_lists` を比較可能性でフィルタしない | typeset.rsの保守的`comparable=false`修正 | 未 |
| D03 | scope.rs | `lookupIgnoringCase` 未移植 | — | 未 |
| D04 | universe.rs | `any`-hijack（gotypesalias=0 legacy）未移植 | — | 未 |
| D05 | named.rs/call.rs/check_lookup.rs | **generic instance method=chunk67/68/69でほぼ完全回収**(Go `expandMethod`相当を遅延実装): (67)直接selection=`named_lookup_method`がinstance→origin methods検索+`method_sig_for_recv`がtype args subst、(68)embedded field経由promotion=`walk_embedded_path`でindex pathを辿り実宣言型を特定、(69)interface充足=`Checker::expand_instance_methods`がorigin methodを`obj_decl`解決後substしinstance.methodsを埋める(`assignable_to`両operandに配線)。tests/instantiation.rs +10。残: `verify_targs`(制約満足)経由のinstance型引数method set比較は未subst(稀) | — | 回収済(67,68,69) |
| D06 | predicates.rs | **`identical_signatures` の型パラメータ置換比較=chunk70で回収**: ジェネリック署名の同一性を「型パラメータのリネームを除いて同一」で判定(Go `predicates.go` `*Signature`)。`subst`は`&mut ObjectArena`/`Context`を要し`identical`が持たないため、物理置換の代わりに `y-tparam → x-tparam` のリネームmap(`HashMap<TypeId,TypeId>`)を`identical_inner`と全再帰ヘルパに通し、y operandを各レベルで写像(subst相当・アロケーション無)。tparam数比較→制約pairwise比較(置換後)→params/results比較。ネストしたジェネリック署名はmapをマージ。tests/predicates.rs +4 | — | 回収済(70) |
| D07 | 全体 | **位置情報統合を段階回収中**。error位置は多くの箇所で既にAST由来(`.pos().0 as u32`)。**chunk82**でobject宣言位置の土台=`ObjectId::set_pos`アクセサ追加+package-levelの`const`/`var`/`type`/`func`構築サイト(resolver.rs)で宣言identのposを配線(`Info.Defs`のobj.pos、redeclarationエラーの位置が正しくなる)。**chunk83**でfunc param/result/named receiverのposを配線(signature_check.rs `new_param_var`に`pos`引数追加、named=`name.pos()`/anonymous・recv=`field.pos()`)。**chunk84**でローカルのposを配線: `:=`(check_assign.rs `short_var_decl`)、range変数(stmt.rs)、type switch束縛変数(stmt.rs、guard識別子pos)、ローカルconst/var/type(decl.rs `decl_stmt`)。**chunk85**でstruct field(struct_check.rs `add_field`に`pos`引数追加=named field識別子/embedded field型pos)・type param(decl.rs `declare_type_param`=`name.pos()`、既存"DEFERRED(D07)"コメント解消)・pkg name(resolver import_decl、alias識別子pos又はpath literal pos、normal/dot両方)を配線。**これで全object種の宣言位置がAST由来に**。残: labels(Labelオブジェクト無し=名前ベース)、`end`(scope endはu32保持済だがobj `end`は未)、多行`cycleError`/mono `local_named_vertex` pos gateのper-object pos活用、line/col解決(consumerがFileSet保持=types crate側はu32 byte offset保持で十分) | 位置情報を使うエラーが要るとき | 大部分回収(82-85) |
| D08 | conversions.rs/assignments.rs | `implements`/`representable` をクロージャ `&|_,_| false` で代用 | Checker.implements/representable | **回収済み（20a/20b/20c）**: クロージャはarena引数を取る形に変更し、`Checker::assignable_to`/`convertible_to`が本物の`implements`/`representable_const`を注入（check_assign.rs） |
| D09 | conversions.rs/check_assign.rs | **`Checker.conversion`(in-placeドライバ)=chunk57で回収**(定数畳み込み/representability rounding/integer→string codepoint/overflowエラー/untyped final type更新)。残: const→型パラメータはconvertible_toフォールバック(per-term cause無)、slice→array version gate(go1.20/1.17)は未配線 | — | 回収済(57) |
| D10 | under.rs | `TypeError` が `type#{id}` 文字列を保持 | typestringをError構築に通す | → Step 40 |
| D11 | infer.rs/call.rs | **step 3（未型付き引数のdefault-type昇格）=chunk62で回収**: `infer`に`untyped_args:&[Option<TypeId>]`+`typ_table:&[TypeId]`引数追加。call.rsの`infer_call`が untyped非nil定数operandを step1から withhold(args=None)し untyped_argsで供給→step3で param が単一TypeParam かつ未推論なら`max_type`畳み込み→`default_type`で確定。`Id(1)`→T=int、混在untyped(int+float)→float64。残: untyped nil(default型無=Go同様skip)、reverse inference | — | 回収済(62) |
| D12 | infer.rs/unify.rs | **Go1.21+ interface inference（`enable_interface_inference`）=chunk63で実装**: `unify.rs`の構造的interfaceマッチングブロック(Go 451-545)+line-338 condガード(`!(enable_interface_inference && is_interface(x))`)を移植、`Unifier::new`のpanic撤去。両interface=comparable一致+terms equal+小method set⊆大method set(共通methodはEXACT nify、ifacePair cycle検出)、片interface=`lookup_field_or_method`で相手の全methodを照合。tests/unify.rs 4件(subset on/off、missing method、concrete実装者)。**chunk64で`infer_call`がdefault有効化**: `call.rs`が`self.allow_version(&go1_21())`を`infer`に渡す(Goの`newUnifier(.., allowVersion(go1_21))`相当、versionなし=現行=true=デフォルト有効)。end-to-end test 1件(`func F[T any](x interface{ Get() T }) T`をGet()int持つ型で呼び→T=int)。残: term subset緩和(Goも未) | unify.rsの構造的interfaceブロック | 回収済(63,64) |
| D13 | lookup.rs/check_lookup.rs | `missingMethod`/`implements` 回収済み（chunk 20a）。残: `assertableTo`/`newAssertableTo`/`interfacePtrError`/`funcString`の本格版、`objDecl`呼出し、comparability version gate | Checker | 一部回収（20a） |
| D14 | api.rs/stmt.rs/resolver.rs/signature_check.rs | **`Info.Scopes`+`Info.Implicits`=chunk72で大部分回収**: (72a)AST crateのscope持ちノード(`File`/`BlockStmt`/`IfStmt`/`SwitchStmt`/`TypeSwitchStmt`/`CaseClause`/`CommClause`/`ForStmt`/`RangeStmt`)に`id:u32`追加+`stamp.rs`で採番(既存Expr/Ident/FuncType idと同機構、`SelectStmt`はGo同様非scope=除外)、(72b)`Info::scopes:HashMap<u32,ScopeId>`+`Checker::record_scope`を全scope開設サイト(file scope=resolver、block=check_block、if/for/range/switch/type switch+case/comm clause=stmt.rs)に配線、`open_scope`が`ScopeId`返却に、tests/scopes.rs +4。(72c)`Info::implicits:HashMap<u32,ObjectId>`+`Checker::record_implicit`、型switch束縛`switch v:=x.(type)`の各CaseClause→narrowed Var配線(stmt.rs)、tests/implicits.rs +2。(72d)AST `Field`に`id:u32`追加+stamp、匿名param(`func(int)`)/無名receiver(`func (T) M()`, 非generic+generic両path)→implicit Var配線(signature_check.rs、Go signature.go)、tests/implicits.rs +2。(72e)残りのscope 2種を回収=`FuncType`→関数scope(`func_body`に`func_type_id`引数追加、両caller `FuncDecl.ty.id`/`FuncLit.ty.id`渡し、body BlockStmtは非記録=Go忠実)+`TypeSpec`→generic型パラメータscope(AST `TypeSpec`に`id:u32`追加+stamp、decl.rs `type_decl`の"type parameters" scope開設で記録、非generic型は非記録)、tests/scopes.rs +4。**これで`Info.Scopes`はGo api.goの全ノード集合(File/FuncType/TypeSpec/Block/If/Switch/TypeSwitch/CaseClause/CommClause/For/Range)を網羅**。(72f)最後のImplicits=import PkgName: AST `ImportSpec`に`id:u32`追加、resolver.rs `import_decl`で name無し`import "unsafe"`→`recordImplicit(spec.id,pkgname)`/alias付き`import u "unsafe"`→`recordDef(alias,pkgname)`(Go resolver.go)。unsafe以外はimporter無で自然に非記録。tests/implicits.rs +2。**Info.Scopes+Info.Implicitsは現状サポート入力について完全記録**。残(D14): `FileVersions`/`StoreTypesInSyntax`と非unsafe package解決自体(D16 importer)、`Sizes`(D23済)/`conf.Error` | importer(D16) | **Scopes/Implicits記録は完全回収(72a-f)**、Importerのみ残 |
| D15 | check.rs(予定) | `handleBailout`/`_aliasAny`/`cleanup`/`pkgPathMap` 未実装 | — | → 随時 |
| D16 | importer.rs/check.rs/resolver.rs | **Importer配線=chunk73aで回収**: `importer`モジュール(`Importer` trait + `ImportCtx`=arena &mut束; アリーナモデルゆえ importパッケージも同一arenaに確保する必要があり trait に arena を渡す)、`Checker::set_importer`/`import_package(path)`(unsafe直/他は importer 呼出し、`take()`で借用衝突回避、path毎キャッシュ)。resolver `import_decl`が任意pathを`import_package`経由で解決しPkgName束縛(Def/Implicit記録)。`pkg.X`値/`pkg.T`型 selector は既存の汎用パスで解決。tests/importer.rs +5(in-memory importer)。**PkgName修飾型/修飾子は完動**。**(73b)組込みsource importer**: `Checker::add_dependency_source(path,files)`で依存パッケージのGoソースを登録→`import_package`が(trait importerより優先で)`check_dependency`=**per-packageステートをsave→依存を`check_files`で再帰チェック(同一arena)→restore**、依存を先にキャッシュ(diamond共有)+`importing`スタックでcycle検出、依存の診断はcallerの`errors`にappend。transitive import解決(`main→p3→p2`)。tests/importer.rs +4(cross-pkg const/type/func、依存error surface、transitive、cycle終了)。**(chunk75)blank import `import _ "path"`**: 早期returnをやめ、`_`は package を resolve(side effect/依存errorをsurface)+`_`PkgName束縛(declareがscope挿入skip)、`unused_imports`が`_`をskip。**(chunk76)pkg-vs-file-scope name clash**: `collect_objects`が`file_scopes:Vec<ScopeId>`を保持し、全decl後に各file-scope名をpkg scopeと照合→衝突は`DuplicateDecl`(`X already declared through import of package "path"`、dot-import objは"through dot-import")。tests/importer.rs +3。**(chunk77)dot-import `import . "path"`**: importパッケージのexportオブジェクトをfile scopeにmerge(`scope::insert_no_reparent`=生insert、他パッケージのobjを再parentしない)、衝突=DuplicateDecl。`dot_imported:HashMap<PackageId,ObjectId>`+`mark_dot_import_use(obj)`(bare identがdot-import objに解決したらそのPkgName usedマーク、expr.rs ident/typexpr type_ident に配線)。unused/clashもdot-importで動く。tests/dot_import.rs +6。残: gcexportdata バイナリローダ、~~dot-import/blank import~~(**chunk75/77**)、~~pkg-vs-file-scope名前衝突~~(**chunk76**)、~~unused-import(D17)~~(**chunk74**)、cgo | (gcexportdata等) | source importer + 全import形態完動(73-77) |
| D17 | resolver.rs/check.rs/call.rs | `package_objects`（3-phase obj_decl）= **回収済(chunk32)**。**`unused_imports`=chunk74で回収**: Checkerに`imports:Vec<ObjectId>`(束縛PkgName、source順)+`used_pkg_names:HashSet<ObjectId>`追加。resolver `import_decl`が束縛PkgNameを`self.imports`にpush、call.rs `selector`のqualified-ident fast-path(`pkg.X`値/型両方が通る)で`used_pkg_names.insert`。`check_files`末尾で`unused_imports()`=imports中`used_pkg_names`に無いものを`UnusedImport`ソフトエラー(`"path" imported and not used`/alias時`"path" imported as name and not used`、name==path末尾要素なら前者)。blank`_`/dot`.`は現状bindされず(D16)→自然に非報告(Goと一致)。`check_dependency`のsave/restoreに両フィールド追加(依存も自前でunused検査)。tests/unused_imports.rs 7件。methodの`hasPtrRecv_`早期フラグは未設定（sigから後で復元） | — | 回収済(32,74) |
| D18 | resolver.rs/decl.rs | Const/Varを`Typ[Invalid]`プレースホルダで生成→decl.rs 23bの`const_decl`/`var_decl`が`set_typ`/`set_val`で実型に置換 | — | **回収済(23b)** |
| D19 | decl.rs | **型パラメータ付き`type T[P ...] U`宣言はchunk35a-declで回収**(`collect_type_params`/`bound`/`declare_type_param`)。残: generic **alias**の型パラメータ未対応、validType(later)省略、`type T U`(Named RHS)のunderlyingがNamed/Aliasに解決される場合はInvalid化(真のサイクル検出はcycles.go)、funcDeclのcycle-guard省略 | cycles.go/generic alias | 一部回収(35a-decl) |
| D20 | signature_check.rs | **func型パラメータ(35c)＋受信者型パラメータ(35e: `unpack_recv`/`collect_recv`/`rparams`)回収済**。残: Signatureに`scope`フィールド無し(bodyは別scopeで再宣言)、`validRecv`(later)、generic alias methodのエラー化 | sig.scope schema | 回収済(35c,35e) |
| D21 | call.rs | 暗黙の型推論(callee sigのtparamsを引数型からinfer)=chunk35d、**明示インスタンス化`f[int](...)`/`f[T1,T2](...)`=chunk71**(`funcInst`)。残: 部分明示+推論補完(`got<want`はCannotInferTypeArgs)、多値単一引数(genericExprList展開)、reverse type inference、`renameTParams`(再帰呼出し)、未型付き引数のdefault昇格(D11) | genericExprList/renameTParams + infer step3(D11) | 一部回収(35d,71) |
| D22 | index.rs/typexpr.rs/call.rs | 型インスタンス化`T[int]`=chunk35a(typexpr)、**関数値の明示インスタンス化`f[int]`(値形・呼出し形)=chunk71**(`is_generic_func_value`→`index_expr`がtrue→`func_inst`。IndexExpr単一/IndexListExpr複数、expr_internal値形とcall_expr呼出し形の両方)。残: 型パラメータoperand(Go の Interface/underIs ブランチ=型集合越しの添字/スライス)未対応 | generics配線 | 一部回収(35a,71) |
| D23 | builtins.rs/sizes.rs | **`sizes.rs`(chunk45)/unsafe.Sizeof・Alignof・Offsetof(chunk46)/unsafe.Add・Slice・SliceData・String・StringData(chunk47)回収済**。**version gate(clear/min/max→go1.21, unsafe.Add/Slice→go1.17, SliceData/String/StringData→go1.20)はchunk56で配線**(`verify_versionf`をdispatch地点で)。残: test専用assert/trace、型パラメータoperand(underIs/applyTypeFunc→underlying直近似)、append([]byte,str...)/copy([]byte,str)のstring特例、hasCallOrRecv追跡、new(expr)値形(go1.26)version gate | generics | 大部分回収(45/46/47/56) |
| D24 | stmt.rs/check_assign.rs/decl.rs | chunk34で型assert/switch、**chunk36で`usage`(declared-and-not-used)回収**、~~comma-ok(map/assert/recv)~~(**chunk40/41/42回収**)。残: type switch束縛変数のcross-clause使用検査、`:=`再宣言(no new vars)、~~`labels.go`2nd pass~~(**chunk39回収**)、~~`isTerminating`(MissingReturn)~~(**chunk37回収**)、~~return mismatch(returnError)~~(**chunk80回収**)/~~named-result shadow(OutOfScopeResult)~~(**chunk81回収**)、~~switch重複case**値**検出(goVal)~~(**chunk43回収**)、~~range func iterator(go1.23)~~(**chunk79回収**: `range_key_val`のSignature arm)、~~go/defer conversion分類~~(**chunk88回収**)、`hasCallOrRecv`、record系no-op。~~n:1多値var`var a,b=f()`~~(**chunk78回収**: `var_decl`が`lhs`リストを取り`init_vars`でtuple/comma-ok unpack、package/local両方)。~~parser quirk: `defer f()`がparenthesized誤判定~~(**chunk87回収**: `parse_call_expr`が`unparen(x.clone())`をポインタ比較していた=常にclone別物→毎回「must not be parenthesized」。`std::ptr::eq(unparen_ref(&x), &x)`に修正、`exprs_eq_ptr`削除)。**chunk88=exprKind機構**: `ExprKind`に`Conversion`追加(既存Expression/Statementに)、`call_expr`が`ExprKind`返却(invalid/ordinary=Statement、typexpr=Conversion、builtin=`PREDECLARED_FUNCS[id].kind`)、`expr_internal`/`raw_expr`/`expr`/`expr_with_hint`が`ExprKind`返却(CallExpr=call kind、ParenExpr=内側kind、他=Expression)。`suspended_call`(go/defer)=Conversion→InvalidGo/InvalidDefer「requires function call, not conversion」/Expression→UnusedResults「discards result of」/Statement→OK。`ExprStmt`=従来の構文的`is_call`近似を廃し`kind==Statement`で許可(conversion `int(x)`/expression-builtin `len(s)`のstatement位置がUnusedExprに=以前は見逃し)。tests/check_files.rs +7。残: `hasCallOrRecv`(record用)、go/defer callのInfo記録(call_expr直呼びでraw_exprのrecord経由せず) | labels.go/initorder | 一部回収(30,34,36,37,39,40,41,42,43,78,79,87,88) |
| D25 | literals.rs/expr.rs | **`exprWithHint`配線=chunk86で回収**: `Checker::expr_with_hint(x,e,hint)`(=`raw_expr(x,e,Some(hint))`)+`raw_expr`/`expr_internal`に`hint:Option<TypeId>`引数追加、CompositeLit armが`hint`を`composite_lit`へ渡す(ParenExprは`None`=go.dev/issue/29316 括弧越しは推論しない)。literals.rsの`indexed_elts`(array/slice要素)・`composite_map`(key/value)が`expr`→`expr_with_hint`に。**struct field(keyed/positional)はGo同様plain `expr`のまま**(ネスト構造体リテラルは明示型必須)。効果: `[][]int{{1,2},{3}}`/`[]Point{{1,2},{3,4}}`/`map[string][]int{"a":{1,2}}`/`map[[2]int]bool{{1,2}:true}`が通る。tests/literals.rs +6。**763 tests pass**。~~map重複key検出(keyVal/visited)~~(**chunk44回収**)。残: Info recording(ネストリテラル要素型)。**struct literalのdriverテストはchunk33aで回収済** | — | 回収済(33a,44,86) |
| D26 | interface_check.rs/struct_check.rs | **embedded field validity check(struct_check.rs)=chunk61で回収**(`check_embedded_field`: deref後 unsafe.Pointer/Pointer/pointer-to-interface/型パラメータを`InvalidPtrEmbed`/`MisplacedTypeParam`で拒否、`later`で遅延)。残: interface method recv未設定(sig.recv=None)、sortMethods省略、method型パラメータ拒否、`def`-based recv命名 | — | 一部回収(61) |
| D27 | typexpr.rs | **制約満足検査=chunk35bで回収**(`verify_targs`→`implements(constraint=true)`, InvalidTypeArgソフトエラー)。**`mono.recordInstance`=chunk58で配線**(typexpr/call両サイト)。残: go1.20 comparability version gate(満足扱い)、`tpar.iface()`明示呼出し省略 | version | 回収済(35b,58) |
| D28 | mono.rs/infer.rs/call.rs | **mono.go(単相化サイクル検出)=chunk58で移植**(`MonoGraph`+`monomorph`)。残: source経由end-to-endサイクル検出はinfer(D11: `T:=*T`等パラメータ化推論結果拒否)+明示`f[T]()`(D21)未対応でブロック、`record_canon`配線(generic method receiver)未、`local_named_vertex`のpos gateはD07で縮退 | infer step3(D11)/明示instantiation(D21)/pos(D07) | 一部回収(58) |
| D29 | format.rs | **format.go(メッセージ整形)=chunk59で移植**(`strip_annotations`/`ndigits`/`qualifier`/`type_list_str`/`operand_list_str`、`type_str`を修飾子経由に)。残: `qualifier`の`pkgPathMap`/`markImports`重複解決(同名2パッケージ→フルpath引用)はimporter待ち、`trace`/`dump`(stdoutデバッグ)は実pos+`check.indent`要で省略、`tpSubscripts`は対応物無 | importer(D16)/pos(D07) | 一部回収(59) |

---

## 9. 困ったとき（まぬけ用チェックリスト）

- **コンパイルが通らない**: まずエラーメッセージを読む。`borrow` 系なら §5.1。「`arena` を2回借りた」なら先にローカルへコピー。
- **どう書くか分からない関数がある**: 同種の既存 `*.rs` を `Read` して形をまねる。それでも無理なら §3 のdeferralにして次へ。
- **Goのコードの意味が分からない**: そのGo関数のコメント（`//`）を読む。types2は丁寧にコメントされている。
- **テストの書き方/ASTの組み方が分からない**: 既存 `tests/operand.rs`, `tests/conversions.rs`, `tests/typestring.rs` を見る。パースは `guff::parser`。
- **1ステップが大きすぎる**: 遠慮なく自分でさらに小さく割る（例: 25aをさらに前半/後半に）。**ただし1コミット＝1〜2ファイル＝ビルド通る状態、は厳守。**
- **テスト数が減った**: 何か壊した。直前の変更を見直す。`git diff` で確認。

---

## 付録A: よく使うコマンド
```bash
# 毎回最初に（cargoはPATHに無い）
. "$HOME/.cargo/env"
cd /Users/dakimura/projects/src/github.com/dakimura/me/projects/guff

# ビルド & テスト
cargo build -p guff-types
cargo test  -p guff-types

# Goソースを見る
ls /Users/dakimura/sdk/go1.26.4/src/cmd/compile/internal/types2/
```

## 付録B: 主要Goファイルの行数（移植の重さの目安）
| ファイル | 行数 | 対応ステップ |
|---------|-----|------|
| expr.go | 1458 | 25 (5分割) |
| builtins.go | 1124 | 29 (3分割) |
| call.go | 991 | 26–27 |
| decl.go | 885 | 23 (2分割) |
| stmt.go | 842 | 30 (4分割) |
| resolver.go | 753 | 22 |
| check.go | 664 | 18 (3分割), 32 |
| assignments.go | 603 | 既存＋Step 30a |
| typexpr.go | 537 | 21 (2分割) |
| api.go | 485 | 18a |
| index.go | 455 | 28 |
| const.go | 306 | 20b/25b |
| errors.go | 256 | 19 |
| format.go | 183 | 19/40 |
| recording.go | 175 | 37 |

---

*この計画書はchunk 17完了時点で作成。進捗に合わせて §2 と §7 のチェックボックス、§8 のdeferral表を更新していくこと。*
