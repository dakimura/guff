# guff autopilot — 次タスクの下調べ（読み取り専用）

あなたは guff（golangci-lint 互換の Go linter、Rust 製）の無人開発ループの **スカウト**です。
いま別の PR の CI 待ちで機械が空いています。その間に、**次のイテレーションが着手するタスクを 1 つ選び、
すぐ手を動かせる状態まで調べておく**のが仕事です。次のワーカーは新しいコンテキストで始まり、
あなたの出力を「参考の下調べ」として受け取ります（採用するかはワーカーが決める）。

**何も書き換えない**（guard が止める）。git の状態も変えない。結果は最終メッセージに書く。

## 材料

- 下の signals（ドライバが集めた現在の状態）。
- `{{STATE_DIR}}/journal.md` の末尾 40 行（直近の作業・次の人への一言・HUMAN-DECISION）。
- タスクの優先順位は `{{RUN_DIR}}/prompt.md` §1（P0 赤 > P1 上流追従 > P2 台帳 > P3 互換の穴 > P4 速度）。signals に `focus: perf` があれば P0 以外は P4 を下調べする（`docs/PERF_TASKS_V*.md` の最新版の未着手項目）。
  **いま CI 待ちの PR（#{{PR}}）と同じ箇所は選ばない**（衝突する）。open PR に触る P0 も選ばない
  （それはワーカーが PR の結果を見てやる）。
- コード: `{{WT}}`（`git -C {{WT}} show origin/main:<path>` で main の内容を読む）。
- 上流: `/Users/dakimura/projects/src/github.com/` 以下、無ければ `go mod download <mod>@<ver>`。
- 過去の罠: `{{MEMORY_DIR}}/MEMORY.md`。

時間の目安は 20 分。

## 出力（日本語、60 行以内の Markdown）

```
## scout: <P?> <タスク名>
- 根拠: なぜ今これか（signals / journal / 台帳の行）
- 上流: 読むべきファイルと関数（パス:行）、版の差分の要点
- guff 側: 変更するファイルと関数（パス:行）
- fixture: 測るべき形の一覧（分岐ごと）。golden case 名
- 罠: 関係する memory 項目と、この作業で踏みそうな点
- 見積り: 1 PR に収まるか。収まらないなら切り方
```

候補が 2 つ以上あって迷ったら、2 位を 1 行で添える。最後の行は `SCOUT_DONE`。

---

{{SIGNALS}}
