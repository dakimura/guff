#!/usr/bin/env python3
"""PreToolUse hook for autopilot sessions: refuse the Bash commands and file writes
that have cost this repository real work before, or that would let a session grade
its own homework.

    guard.py                  # worker (the default)
    guard.py --role readonly  # reviewer / scout: no git or GitHub mutations, no edits

Exit 2 + a message on stderr blocks the call and shows the message to the model,
which then adjusts. Anything not matched passes through untouched.
"""
import json
import re
import sys

# The driver's own records. A session that could write these could forge a gate pass
# or a review approval, so nobody but the driver / gate.sh writes them.
PROTECTED = r"\.guff-autopilot/(gates|reviews|run|main-health|lock\.d)\b"
WRITEY = r"(>|\btee\b|\bcp\b|\bmv\b|\btouch\b|\bsed\s+-i|\bperl\s+-[a-z]*i|\bpython3?\b|\brm\b|\bln\b|\bchmod\b)"

WORKER_RULES = [
    (r"\bgit\s+push\b[^\n;&|]*\s(--force\b|-f\b|--force-with-lease\b|\+)",
     "force-push は禁止。CI の緑が古い run のものになる。追加コミットで直す。"),
    (r"\bgit\s+push\b[^\n;&|]*[\s:](main|master)(\s|$|[;&|])",
     "main への直接 push は禁止。autopilot/<slug> ブランチから PR を出す。"),
    (r"\bgit\s+tag\b|\bgit\s+push\b[^\n;&|]*(--tags|refs/tags/|\sv[0-9])|\bgh\s+release\s+(create|delete|edit|upload)\b",
     "タグと release はドライバ（release.sh）の仕事。ワーカーは作らない。"),
    (r"\bgh\s+pr\s+(merge|review)\b",
     "マージとレビュー判定はドライバ（run.sh）の仕事。PR を出したら AUTOPILOT_RESULT を出して終わる。"),
    (r"\bgh\s+(api\s+\S*rulesets|repo\s+(edit|delete))\b",
     "リポジトリ設定は変更しない。"),
    (r"\brm\s+(-[a-zA-Z]*[rR][a-zA-Z]*|--recursive)\b",
     "再帰 rm は禁止。一時物は mktemp -d の新ディレクトリに作り、古いものは残す。"),
    (r"\bgit\s+(reset\s+--hard|clean\s+-[a-zA-Z]*f)",
     "git reset --hard / git clean -f は禁止。必要なら git stash push -u。"),
    (r"\bcargo\s+fmt\b",
     "cargo fmt は禁止（リポジトリが未整形で 640 ファイル書き換わる）。"),
    (r"\bgit\s+commit\b[^\n]*--no-verify",
     "--no-verify は使わない。"),
]

READONLY_RULES = [
    (r"\bgit\s+(commit|push|switch|checkout|reset|stash|merge|rebase|cherry-pick|revert|tag|branch\s+-[dDmM]|add|rm|mv|restore|clean|worktree|am|apply)\b",
     "読み取り専用セッション: git の状態を変えるコマンドは使えない（git show / diff / log / grep は可）。"),
    (r"\bgh\s+(pr\s+(create|merge|comment|review|edit|close|reopen|ready)|issue\s+(create|comment|edit|close)|release|repo\s+(edit|delete)|api\b[^\n]*-X\s*(POST|PATCH|PUT|DELETE))",
     "読み取り専用セッション: GitHub への書き込みはできない。結果は最終メッセージに書く（ドライバが投稿する）。"),
    (r"\bsed\s+-i|\bperl\s+-[a-z]*i\b|\brm\b|\bcargo\s+fmt\b",
     "読み取り専用セッション: ファイルを書き換えない。"),
]

ALWAYS_RULES = [
    (PROTECTED + r"[^\n]*" + WRITEY + "|" + WRITEY + r"[^\n]*" + PROTECTED,
     "ドライバの記録（gates/・reviews/・run/・main-health）は書き換えない。ゲートは gate.sh を走らせて記録させる。"),
]


def main() -> int:
    role = "worker"
    if "--role" in sys.argv:
        role = sys.argv[sys.argv.index("--role") + 1]
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    tool = data.get("tool_name")
    inp = data.get("tool_input") or {}

    if tool in ("Write", "Edit", "NotebookEdit", "MultiEdit"):
        path = inp.get("file_path") or inp.get("notebook_path") or ""
        if role == "readonly":
            print("autopilot guard: 読み取り専用セッション: ファイルは書かない。結果は最終メッセージに書く。", file=sys.stderr)
            return 2
        if re.search(PROTECTED, path):
            print("autopilot guard: ドライバの記録（gates/・reviews/・run/）は書き換えない。", file=sys.stderr)
            return 2
        return 0

    if tool != "Bash":
        return 0
    cmd = inp.get("command") or ""
    rules = ALWAYS_RULES + (READONLY_RULES + WORKER_RULES if role == "readonly" else WORKER_RULES)
    for pat, why in rules:
        if re.search(pat, cmd):
            print(f"autopilot guard: {why}", file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
