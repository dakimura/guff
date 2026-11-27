#!/usr/bin/env python3
"""PreToolUse hook for autopilot workers: refuse the few Bash commands that
have cost this repository real work before (see the reasons below).

Exit 2 + a message on stderr blocks the call and shows the message to the
model, which then adjusts. Anything not matched passes through untouched.
"""
import json
import re
import sys

RULES = [
    (r"\bgit\s+push\b[^\n;&|]*\s(--force\b|-f\b|--force-with-lease\b|\+)",
     "force-push は禁止。CI の緑が古い run のものになる。追加コミットで直す。"),
    (r"\bgit\s+push\b[^\n;&|]*[\s:](main|master)(\s|$|[;&|])",
     "main への直接 push は禁止。autopilot/<slug> ブランチから PR を出す。"),
    (r"\bgit\s+tag\b|\bgit\s+push\b[^\n;&|]*(--tags|refs/tags/|\sv[0-9])|\bgh\s+release\s+(create|delete|edit|upload)\b",
     "タグと release はドライバ（release.sh）の仕事。ワーカーは作らない。"),
    (r"\bgh\s+pr\s+merge\b",
     "マージはドライバ（scripts/autopilot/run.sh）の仕事。PR を出したら AUTOPILOT_RESULT を出して終わる。"),
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


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except Exception:
        return 0
    if data.get("tool_name") != "Bash":
        return 0
    cmd = (data.get("tool_input") or {}).get("command") or ""
    for pat, why in RULES:
        if re.search(pat, cmd):
            print(f"autopilot guard: {why}", file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
