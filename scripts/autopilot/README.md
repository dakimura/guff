# autopilot — 無人開発ループ

```
scripts/autopilot/run.sh start    # 起動（スクリプトを ~/.guff-autopilot/run にコピーして detached 実行）
scripts/autopilot/run.sh status   # 現在の状態・main health・直近のセッション・ドライバのメモ
scripts/autopilot/run.sh stop     # 今のイテレーションが終わったら止める（--now で即停止）
scripts/autopilot/run.sh prompt   # 次のワーカーに渡るプロンプトを表示
scripts/autopilot/run.sh release-check  # release の時期か・次のバージョン
scripts/autopilot/run.sh release  # 次のステップで release を切らせる（周期・CI 条件を 1 回だけ無視）
```

## 構成

```
run.sh (ドライバ, モデル無し) ─ loop ─────────────────────────────────────────────────┐
  0. release.sh: 周期が来ていて main が緑なら release（bump PR → tag → release.yml → Formula）│
  1. worktree (../guff-autopilot) を origin/main に揃える／target/debug の肥大を掃除          │
  2. ワーカー（claude -p, 新しいコンテキスト, prompt.md + signals.sh + scout brief）          │
       └ 1 タスク: P0 赤 > P1 上流追従 > P2 台帳 > P3 互換の穴 > P4 速度                      │
       └ 実装 → gate.sh --quick で回す → gate.sh --keep-going を 1 回（ツリーハッシュで記録）→ PR │
  3. PR の CI 待ち（トークン不要）。その間に読み取り専用セッションを 2 つ:                   │
       reviewer（review-prompt.md）: その head を approve / changes。changes は PR にコメント │
       scout   （scout-prompt.md, sonnet）: 次タスクの下調べ → next-task.md → 次の signals   │
  4. マージ判定: CI 全緑 ∧ reviewer approve ∧（コード変更なら）head ツリーのフルゲート記録が  │
     全段 PASS（main でも赤い段は known-red として可）→ squash merge                        │
     満たさなければ PR を残し、理由をコメント → 次のワーカーの P0                            │
  5. main health: マージ後の main で gate.sh --only build,regress（CI に無い段だけ）          │
  6. 利用上限に当たったらリセットまで寝て再開 ──────────────────────────────────────────────┘
```

- **判断はドライバ、作業はモデル**。マージ・タグ・release・ゲートの合否はモデルが決めない。
  `guard.py`（PreToolUse hook）がワーカーの `gh pr merge`/`review`・main への push・force-push・
  タグ・`rm -r`・`reset --hard`・`cargo fmt` と、ドライバの記録（`gates/` `reviews/` `run/`）への
  書き込みを拒否する。reviewer / scout は `--role readonly`（git / GitHub の変更とファイル編集を全部拒否）。
- ゲートの記録 `~/.guff-autopilot/gates/<tree>.tsv` は「gate.sh が走り始めたときの作業ツリー」の
  ツリーハッシュで引く。ワーカーがゲート後にコードを変えて push すると記録が合わず、マージされない。
- ローカルの OSS nightly tier は回さない（メモリ不足で落ちる。main への push で CI が回す）。
- 人が見るべきこと（journal の `NEEDS-HUMAN`、blocked、ドライバの停止）は macOS の通知で知らせる。
- 状態は `~/.guff-autopilot/`：`journal.md`（ワーカーの記録）、`driver-notes.md`、`iterations.tsv`
  （全セッションの所要時間・API 換算コスト）、`logs/`、`reviews/`、`scout/`、`gates/`、`main-health.tsv`。
- 実行中のスクリプトはコピーなので、このディレクトリを編集しても走っているループには
  影響しない（反映は `stop` → `start`）。
- 人間の判断は journal に `### HUMAN-DECISION <日付> <件名>` で追記すると、次のワーカーが最優先で拾う。
