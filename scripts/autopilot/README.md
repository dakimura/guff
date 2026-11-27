# autopilot — 無人開発ループ

```
scripts/autopilot/run.sh start    # 起動（スクリプトを ~/.guff-autopilot/run にコピーして detached 実行）
scripts/autopilot/run.sh status   # 現在の状態・直近のイテレーション・ドライバのメモ
scripts/autopilot/run.sh stop     # 今のイテレーションが終わったら止める（--now で即停止）
scripts/autopilot/run.sh prompt   # 次のワーカーに渡るプロンプトを表示
scripts/autopilot/run.sh release-check  # release の時期か・次のバージョン
scripts/autopilot/run.sh release  # 次のステップで release を切らせる（周期・CI 条件を 1 回だけ無視）
```

## 構成

```
run.sh (ドライバ, モデル無し) ── loop ──────────────────────────────────────────┐
  1. worktree (../guff-autopilot) を origin/main の detached HEAD に揃える         │
  2. signals.sh で状態収集（main CI・open PR・上流の新版・台帳・journal）          │
  3. claude -p（ワーカー, 新しいコンテキスト）に prompt.md + signals を渡す         │
        └ 1 タスク選択(P0 赤 > P1 上流追従 > P2 台帳 > P3 互換の穴 > P4 速度)       │
        └ 実装 → gate.sh（CI と同じ全 tier + regress）→ reviewer subagent → PR     │
        └ journal 追記・memory 更新 → `AUTOPILOT_RESULT: pr-open #N`               │
  4. ドライバが PR の CI を待つ（トークン不要）                                    │
        全チェック緑 & head 一致 → squash merge / 赤 → 開いたまま次の P0 へ        │
  5. 利用上限に当たったらリセットまで寝て再開 ────────────────────────────────────┘
```

- **マージするのはドライバだけ**。ワーカーの `gh pr merge`・main への push・force-push・
  `rm -r`・`git reset --hard`・`cargo fmt` は `guard.py`（PreToolUse hook）が拒否する。
- **定期 release もドライバが行う**（`release.sh`、モデル無し）。各イテレーションの前に判定し、
  前回タグから 7 日以上（`AUTOPILOT_RELEASE_DAYS`）・その間にマージがある・main の head の CI が全部緑
  （前回 release を試す Action self-test は除く）なら: version bump PR（緑ならマージ）→ `v<ver>` タグ →
  release.yml の成功を確認 → `Formula/guff.rb` と `docs/INSTALL.md` の sha を更新する PR。
  バージョンは pins.json か `.go-version` が変わっていれば minor、それ以外は patch。
  止めるには `AUTOPILOT_RELEASE=0` で起動。ワーカーはタグ・release を作れない（guard）。
- ローカルの OSS nightly tier は回さない（メモリ不足で落ちる。main への push で CI が回す）。
- 状態は `~/.guff-autopilot/`：`journal.md`（ワーカーの記録）、`driver-notes.md`、
  `iterations.tsv`（所要時間・コスト）、`logs/<iter>.jsonl`（全ログ）、`<iter>.prompt.md`。
- 実行中のスクリプトはコピーなので、このディレクトリを編集しても走っているループには
  影響しない（反映は `stop` → `start`）。
