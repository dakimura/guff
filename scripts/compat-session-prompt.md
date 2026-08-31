# The paste

Paste the block below into a fresh interactive Claude Code session started in
this repository. It works the corpus toward 100 compatible targets, one pull
request at a time, and keeps going until it runs out of room — then you open a
new session and paste it again.

Nothing needs to be remembered between sessions: `corpus/status.json` is the
queue and `docs/COMPAT-HARDENING.md` is the history.

---

guffのコーパスを「100ターゲットでguffとgolangci-lintのfinding集合が一致する」状態に進めてほしい。今の到達点は `./corpus/status.py report` が答える。

作業の単位は1タスク=1プルリク。1つ終えたら次を取って、コンテキストが尽きるまで続けて。

## 進め方

1. `./corpus/status.py next` が次の1件を出す。それをやる。
2. 終わったらブランチを切ってプルリクにする。
3. **CIが全部緑になったら自分でマージしていい。** 私の確認は要らない。ただし:
   - **全部**というのは `unit` / `isolate` / `smoke` / `oss-pr` の 4 つ。`skipping` は数に入らない。
   - マージ前に run の `headSha` がプルリクの head と一致していることを確かめる。force-push すると名前ベースの差分監視は cancelled な run の pass を引き継ぐ。
   - 赤なら直す。落ちた理由が分からないまま再実行しない。
   - `main` のルールセット（`require_extra_approval_for_unattributed_changes`）に弾かれたら、そこで止めて報告する。無理に通さない。
4. マージしたら `git checkout main && git pull` して `./corpus/status.py probe`、次のタスクへ。

CIが長いあいだ、待たずに次のタスクの調査を始めていい —— ただし**前のプルリクのブランチの上で作業しないこと**。`main` に戻ってから新しいブランチを切る。台帳が古いままなので `next` が同じタスクを返すが、それは無視して次のものを取っていい。次の変更が同じ関数・同じgolden・同じdoc節に触るなら、先にマージしてから `main` を引き直したほうが早い。

`compat/run.sh` と `compat/hunt.sh` は同時に走らせないこと（`compat/results/` を共有している）。

## 最初に読むもの

- `docs/COMPAT-HARDENING.md` — このプロジェクトの正典。§4のセッションログは「新しいセッションはこれだけ読めば足りる」ように書いてある。直近2〜3件を読む。
- `corpus/status.json` — 台帳とキュー。`./corpus/status.py report` で表になる。
- `compat/README.md` — tierの仕組み。

## タスクの種類ごとのやり方

**`close <target>`** — 片方のツールだけが出しているfindingがある。**証明されるまでは全部guffの欠陥**として扱う。

1. そのターゲット自身のpatched configで再現する（`compat/results/hunt-*/<target>.config.yml`）。自分で書いたconfigではなく。
2. **Rustを触る前にscratchpadで最小再現を作る。** セッションログで失敗した回は、ほぼ全部ここを飛ばしている。最小再現で**再現しなかったら形のせいではない** —— config、import、プラットフォームを見る。
3. **上流の線を1形から推測せず、複数形で測る。** 1形1宣言で書いて両ツールに通す。直す根拠は「見た規則」であって「推測した規則」ではない。
4. 上流のソースを読む。checkoutは `/Users/dakimura/projects/src/github.com/` の下にある（go-critic、mgechev/revive、timakin/bodyclose、honnef.co/go/tools …）。無いmoduleは `go mod download <mod>@<pin>` 一発。**上流はコメントよりコードを信じる。**
5. guffを直したら、**測った形を全部**fixtureに入れる。壊れていた1形だけではなく。1形しか通さないfixtureは他の分岐を隠す —— これで何セッションも失っている。
6. **そのfixtureをRustの単体テストからも数える。** `assert!(messages.iter().any(|m| m.contains(…)))` は他の形が全部壊れていても緑になる —— S1001 は 7 形のうち 3 形が黙ったまま `any(contains("copy(to, from)"))` を通していたし、unparam も同じ形で 12 形の欠落を隠していた。`assert_eq!(messages.len(), N)` と形ごとの件数で固定する。goldenは golangci-lint のインストールが要るが、`cargo test` はどこでも 3 分で回る。
7. 影響したケースのgoldenを再生成して差分を確認する: `./compat/golden/run.sh --regen --case <case>`。上流も黙るなら差分は出ないはず。

**`measure <target>`** — `./compat/hunt.sh --name <target>` を回す。`--name` を省くと全ターゲットで数時間かかるので必ず付ける。クリーンならそのイテレーションの成果は台帳だけ。

**`adopt <name>`** — `corpus/candidates-100.json` から `corpus/hunt.json` にエントリを足す（`_`始まりのキーは落とす）。そのあと上と同じく測る。どちらかのツールがそのconfigで起動を拒むなら無理に通さず、理由を `corpus/README.md` の除外表と `corpus/status.py` の `EXCLUDED` に書く。それがその回の成果。

## プルリクを出す前に回すゲート

```
./compat/golden/run.sh
./compat/fix/run.sh
./compat/reject/run.sh
cargo test --workspace --locked
```

analyzerが読むものを変えたなら（linterの修正は常にそう）ゲート済みコーパスも回す。**規則を狭めると、一致していたfindingを黙って落とすことがある**:

```
cargo build --release --locked -p guff-lint
./compat/run.sh --oss --tier pr
```

そのあと `./corpus/status.py probe` で台帳を更新して、変更に含める。

## 守ること

- **`main` に直接pushしない。**
- **ゲートを緩めて通さない。** 意図的な乖離なら `compat/allowlists/` か `compat/fix/divergent/` に、**上流に対して測った根拠つきで**入れる。まずguffを直すことを常に優先。
- **このホストで出せない測定を記録しない。** cri-oはlinux専用で、darwinでは両ツールともill-typedになる。測っているのは互換性ではなく環境。
- リビルド後の測定は `--no-cache`。issues cacheのsaltはバージョン文字列なので開発ビルドを区別しない。古い結果は「直っていない」と全く同じ顔をする。
- `cargo test` が始まらないように見えたら `./scripts/target-hygiene.sh --prune`。7日ルールで落ちなければ `rm -rf target/debug`（releaseツリーは残す）。
- コミットとプルリクの文面は、**何が壊れていてどう分かったかを、測った数字と一緒に**書く。このリポジトリのログは1年後に読まれる前提で書かれている。既にあるものに合わせて。
- **互換性の修正には単体テストも足す。** 新しい述語・新しいガードには、上流を知らなくても何を主張しているか読めるRustのテストを1つ以上（続き108の `ctrlflow` は制御構造12形を `#[test]` で固定した）。goldenだけに頼ると、golangci-lintが手元に無い環境では何も守られない。
- guffの挙動が変わったなら `docs/COMPAT-HARDENING.md` の§4に続き番号でエントリ、`docs/SESSION-LOG.md` に1行。何も見つからなかった測定なら両方とも不要。

## 詰まったとき

タスクが間違っていると分かったら —— 既にクリーンだった、findingが上流のバグだった、直すには無い部品が要る —— **プルリクにそう書いて止まる。** 理由を測定つきで残すのは立派な成果。推測で埋めるのはそうではない。

私は見ているので、判断に迷ったら聞いて。

## 状況報告

1タスク終わるごとに、次に進む前に短く報告して: 何が壊れていたか、どう測ったか、台帳がいくつ動いたか。長い解説はプルリクに書けばいいので、ここでは3〜4行で。
