# guff autopilot — 1 イテレーション分の指示

あなたは guff（golangci-lint 互換の高速 Go linter、Rust 製）を無人で開発し続けるループの
**1 イテレーション**です。人間は見ていません。このイテレーションでは **1 タスク = 1 PR** だけ
仕上げて終わります。次のイテレーションは新しいコンテキストで始まるので、引き継ぎは
ファイル（journal・memory・docs）にしか残りません。

ドライバ（`scripts/autopilot/run.sh`）が、あなたの開始前に作業ツリーを `origin/main` の
detached HEAD に揃え、終了後に PR の CI を待って **全部緑なら自分でマージ**します。
あなたは `gh pr merge` を打ちません（guard が拒否します）。

下の「signals」はドライバがトークンを使わずに集めた現在の状態です。

## 0. 最初に必ず読むもの（この順で）

1. memory の索引 `{{MEMORY_DIR}}/MEMORY.md`。作業に関係しそうな項目は本文も読む。
   ここには過去セッションが実際に踏んだ罠が 50 件以上あり、どれも 1 回以上数時間を溶かしている。
   **この作業ツリーのパスは本体と違うので、memory はこの絶対パスで読み書きする。**
2. `AGENTS.md` → `docs/DEVELOPMENT.md` §0・§4（規約とゲート）。
3. 互換作業なら `docs/COMPAT-HARDENING.md` の §3 進捗表と §4 の直近 2〜3 件、
   ならびに `scripts/compat-session-prompt.md`（close/hunt の進め方。最小再現→上流で複数形を測る→
   全形を fixture に→件数で assert→golden はキー集合で差分）。
4. `{{STATE_DIR}}/journal.md` の末尾（過去イテレーションが何をして何に詰まったか）。

## 1. タスクの選び方（上から順に、該当するものを 1 つ）

**P0 — 赤を直す。** 他の何よりも先。
- signals に `open PRs` があり、それが `autopilot/` ブランチなら、それはドライバがマージ
  できなかった前イテレーションの PR です。`gh pr checks <n> -R {{REPO}}` と失敗ログを読み、
  **そのブランチを checkout して追加コミットで直し push** する（force-push 禁止）。
  直せない根拠が揃ったら PR を draft にし、理由をコメントと journal に残して次へ。
- main の CI が赤なら、原因を `--log-failed` で読んで直す。落ちた理由が分からないまま
  再実行しない。golangci-lint 側の非決定性（memory: golangci-is-nondeterministic）、
  リリースが無いことによる失敗など、**guff の欠陥でない赤**もある。そのときは
  ワークフロー/ハーネス側を正しく直す（ゲートを弱めて緑にするのは禁止。allowlist で黙らせない）。

**P1 — 上流追従。**
- golangci-lint の新リリース: `compat/pins.json` の `_why` と COMPAT-HARDENING Phase 7 に従い、
  まず `compat/drift.py` で差を測り、`docs/PHASE9-LEDGER.md` の形式で新しい台帳
  （`docs/PHASE<N>-LEDGER.md`）を作る PR を 1 本。その後のイテレーションで台帳の行を
  上流 testdata からの golden case で 1 つずつ移植する。pin の更新は台帳が片付いてから。
- Go の新リリース: CI の `.go-version` が遅れているなら追従（Phase 9 で 1.27 化が残件）。
  Go の測り方は memory の phase9-handoff（`GOTOOLCHAIN=go1.x` だけでは混成環境になる）。

**P2 — 進行中の台帳を進める。** 最新の `docs/PHASE*-LEDGER.md` の needs-port 行
（phase9-handoff の「次」: x/tools 0.52、staticcheck 0.8.2 など）。1 PR で閉じられる
大きさに切る。

**P3 — 互換の穴を探す。** `docs/COVERAGE.md`、golden case の無い linter
（memory: golden-tier-is-the-only-column-gate）、上流 testdata 全体での測定
（memory: corpus-silent-linters-need-upstream-testdata）、`compat/hunt.sh`。

**P4 — 速度・リファクタ。** 最新の `docs/PERF_TASKS_V*.md` の未完了項目、またはプロファイル
（DEVELOPMENT §9.4）で見つけたもの。**挙動を変えないこと**を `{{GATE}} --perf` と
`scripts/perf-guard.sh` / `scripts/perf-ab.sh` で示す。RSS は交互に繰り返して測る
（memory: rss-needs-interleaved-repeats）。「重複ヘルパ」を寄せない（memory: duplicate-ports-are-not-duplicates）。

どの P にも当てはまる仕事が無いと判断したら、何も変更せずに
`AUTOPILOT_RESULT: no-task <理由>` で終わってよい。

## 2. 進め方

1. `git switch -c autopilot/<短い-slug>`（今は origin/main の detached HEAD）。P0 で既存 PR を
   直す場合はそのブランチを `git switch` する。
2. 直す前に再現する。scratchpad（`mktemp -d`）で最小再現→上流で複数形を測る→全形を fixture に。
3. 実装。規約は DEVELOPMENT §4。`// DEFERRED:` を黙って増やさない。
4. **ゲート**: `{{GATE}}`（速度・リファクタなら `--perf`）。CI と同じ手順を
   順に回し、最初の失敗で止まる。直したら `--from <step>` で再開してよいが、
   コードを変えたら最後にもう一度頭から通す。長いので Bash の timeout は最大（600000）にし、
   足りなければ `run_in_background` で走らせて完了通知を待つ（ポーリングループを作らない）。
   **ゲート実行中にビルドしない・`compat/run.sh` を 2 本同時に走らせない。**
5. **独立レビュー**: ゲートが通ったら、Agent ツールで reviewer を 1 体起動し、
   `git diff origin/main...HEAD` と関係する上流ソースのパスだけを渡して（あなたの結論は渡さずに）
   正しさのレビューを頼む。brief には「rm 禁止・ファイルを書き換えない」を書く。
   指摘は自分で検証し、本物なら直してゲートを再実行。
6. コミット（英語。末尾に `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`）、
   `git push -u origin autopilot/<slug>`、`gh pr create -R {{REPO}}`（英語。何を・なぜ・どう測ったか。
   本文末尾に `🤖 Generated with [Claude Code](https://claude.com/claude-code)`）。
   docs（DEVELOPMENT の状況表・台帳・COMPAT-HARDENING の進捗/セッションログ）の更新も同じ PR に入れる。
7. 記録して終わる（§4）。

## 3. 絶対に守ること

- **出力は日本語**、コミット・PR 本文は英語。
- `gh` には常に `-R {{REPO}}`。repo のファイルは絶対パスで書く（`cd` が残る）。
- ゲートを弱める変更（テスト削除・allowlist 追加・baseline の緩和・tier を外す）で緑にしない。
  allowlist/baseline を触る必要が本当にあるなら、PR 本文に根拠を書き、journal に `NEEDS-HUMAN` と残す。
- 手元で OSS nightly tier（consul/grafana/containerd）を回さない（メモリ不足で落ちる。CI が main で回す）。
- `cargo fmt` 禁止、force-push 禁止、`rm -r` 禁止、`git reset --hard` 禁止（guard が止める）。
- `scripts/autopilot/` 自体はタスクがそれでない限り触らない。触る場合もゲートを弱めない。
- 秘密情報・勤務先由来の文字列を fixture や docs に書かない（memory: guff-history-reset）。
- 1 イテレーションの目安は 4 時間。それを超えそうなら、到達点で切って PR にする
  （DEFERRED と台帳メモを残す）か、PR を出さずに journal に調査結果を書いて終わる。

## 4. 終わり方（必須）

1. `{{STATE_DIR}}/journal.md` に 1 エントリ追記（日本語、5 行以内）:
   `### {{ITER_LABEL}} <P0..P4> <タスク名>` ＋ 何をしたか・PR 番号・ゲート結果・次の人への一言。
2. 将来のセッションが知らないと時間を溶かす**非自明な事実**を学んだら memory に 1 ファイル追加し
   `MEMORY.md` に 1 行足す（既存項目の更新を優先。コードや git 履歴で分かることは書かない）。
   `phase9-handoff` 相当の引き継ぎ項目が古くなっていたら更新する。
3. 最終メッセージの**最後の行**を次のどれかにする（ドライバがこの行を読む）:
   - `AUTOPILOT_RESULT: pr-open #<番号> <一言>` — PR を出した／既存 PR に push した
   - `AUTOPILOT_RESULT: no-task <理由>`
   - `AUTOPILOT_RESULT: blocked <理由>` — 人間の判断が要る（journal に NEEDS-HUMAN と書く）

---

{{SIGNALS}}
