#!/usr/bin/env python3
"""Patch a golangci config for stable finding-set diffs.

1. Sets top-level:
     issues.max-issues-per-linter: 0
     issues.max-same-issues: 0
   so identical-message truncation cannot rotate keys.

2. Strips Go ``.so`` custom plugins from ``linters.enable`` /
   ``linters.disable`` and removes matching ``linters.settings.custom``
   entries that have a ``path:`` (goplugin) without ``type: module``.

   guff cannot load golangci goplugins; on Darwin/CI the ``.so`` files are often
   missing and golangci-lint fails at startup (e.g. k8s ``logcheck.so``). Dropping
   them from the shared run config keeps both tools on the overlapping enable-set.

Line-oriented (not full YAML round-trip) so configs with tabs / unusual
scalars (kubernetes hack/golangci.yaml) stay intact.

Usage:
  python3 corpus/patch_unlimited_issues.py INPUT.yml -o OUTPUT.yml
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def is_top_level(s: str) -> bool:
    return bool(s.strip()) and not s.startswith((" ", "\t")) and not s.lstrip().startswith("#")


def indent_width(line: str) -> int:
    return len(line) - len(line.lstrip(" \t"))


def patch_issue_caps(text: str) -> str:
    lines = text.splitlines(keepends=True)
    issues_idx = None
    for i, line in enumerate(lines):
        if re.match(r"^issues:\s*(?:#.*)?$", line):
            issues_idx = i
            break

    keys = {
        "max-issues-per-linter": "  max-issues-per-linter: 0\n",
        "max-same-issues": "  max-same-issues: 0\n",
    }

    if issues_idx is None:
        if lines and not lines[-1].endswith("\n"):
            lines[-1] = lines[-1] + "\n"
        lines.append("\n")
        lines.append("issues:\n")
        lines.append(keys["max-issues-per-linter"])
        lines.append(keys["max-same-issues"])
        return "".join(lines)

    end = issues_idx + 1
    while end < len(lines) and not is_top_level(lines[end]):
        end += 1

    block = lines[issues_idx + 1 : end]
    found = {k: False for k in keys}
    new_block: list[str] = []
    for line in block:
        replaced = False
        for k, replacement in keys.items():
            if re.match(rf"^  {re.escape(k)}\s*:", line):
                new_block.append(replacement)
                found[k] = True
                replaced = True
                break
        if not replaced:
            new_block.append(line if line.endswith("\n") else line + "\n")

    for k, replacement in keys.items():
        if not found[k]:
            new_block.insert(0, replacement)

    return "".join(lines[: issues_idx + 1] + new_block + lines[end:])


def find_custom_goplugins(text: str) -> list[str]:
    """Return custom linter names that look like golangci goplugins (.so path)."""
    lines = text.splitlines()
    # Locate linters.settings.custom
    custom_idx = None
    custom_indent = None
    for i, line in enumerate(lines):
        if re.match(r"^(\s+)custom:\s*(?:#.*)?$", line):
            # Must be under settings (indent > 0)
            custom_idx = i
            custom_indent = indent_width(line)
            break
    if custom_idx is None or custom_indent is None:
        return []

    stripped: list[str] = []
    i = custom_idx + 1
    while i < len(lines):
        line = lines[i]
        if not line.strip() or line.lstrip().startswith("#"):
            i += 1
            continue
        ind = indent_width(line)
        if ind <= custom_indent:
            break
        # Entry name at custom_indent + 2 (approximately): "      logcheck:"
        m = re.match(r"^(\s+)([A-Za-z0-9_-]+):\s*(?:#.*)?$", line)
        if m and indent_width(line) == custom_indent + 2:
            name = m.group(2)
            entry_indent = indent_width(line)
            j = i + 1
            has_path = False
            is_module = False
            while j < len(lines):
                el = lines[j]
                if el.strip() and not el.lstrip().startswith("#"):
                    eind = indent_width(el)
                    if eind <= entry_indent:
                        break
                    if re.match(r"^\s+path\s*:", el):
                        has_path = True
                    if re.match(r"^\s+type\s*:\s*module\b", el):
                        is_module = True
                j += 1
            if has_path and not is_module:
                stripped.append(name)
            i = j
            continue
        i += 1
    return stripped


def strip_enable_names(text: str, names: set[str]) -> str:
    """Remove stripped plugin names from top-level ``enable`` / ``disable`` lists only.

    Must not touch ``exclusions.rules[].linters`` entries — those are handled by
    dropping whole rules that become invalid after plugin removal.
    """
    if not names:
        return text
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    in_target_list = False
    list_indent = None
    for line in lines:
        if re.match(r"^(\s+)(enable|disable):\s*(?:#.*)?$", line):
            # Only the linters.enable / linters.disable keys (indent typically 2).
            ind = indent_width(line)
            if ind <= 4:
                in_target_list = True
                list_indent = ind
                out.append(line if line.endswith("\n") else line + "\n")
                continue
        if in_target_list:
            if line.strip() and not line.lstrip().startswith("#"):
                ind = indent_width(line)
                if list_indent is not None and ind <= list_indent:
                    in_target_list = False
                    list_indent = None
                else:
                    m = re.match(r"^(\s*)-\s+([A-Za-z0-9_-]+)\s*(?:#.*)?$", line)
                    if m and m.group(2) in names:
                        continue
        out.append(line if line.endswith("\n") else line + "\n")
    return "".join(out)


def strip_custom_entries(text: str, names: set[str]) -> str:
    if not names:
        return text
    lines = text.splitlines(keepends=True)
    custom_idx = None
    custom_indent = None
    for i, line in enumerate(lines):
        if re.match(r"^(\s+)custom:\s*(?:#.*)?$", line):
            custom_idx = i
            custom_indent = indent_width(line)
            break
    if custom_idx is None or custom_indent is None:
        return text

    out = lines[: custom_idx + 1]
    i = custom_idx + 1
    kept_any = False
    while i < len(lines):
        line = lines[i]
        if line.strip() and not line.lstrip().startswith("#"):
            ind = indent_width(line)
            if ind <= custom_indent:
                break
        m = re.match(r"^(\s+)([A-Za-z0-9_-]+):\s*(?:#.*)?$", line)
        if (
            m
            and line.strip()
            and not line.lstrip().startswith("#")
            and indent_width(line) == custom_indent + 2
        ):
            name = m.group(2)
            entry_indent = indent_width(line)
            j = i + 1
            while j < len(lines):
                el = lines[j]
                if el.strip() and not el.lstrip().startswith("#"):
                    if indent_width(el) <= entry_indent:
                        break
                j += 1
            if name in names:
                i = j
                continue
            kept_any = True
            out.extend(lines[i:j])
            i = j
            continue
        out.append(line if line.endswith("\n") else line + "\n")
        i += 1

    out.extend(lines[i:])

    if not kept_any:
        out = [ln for ln in out if not re.match(r"^(\s+)custom:\s*(?:#.*)?$", ln)]

    return "".join(out)


def strip_exclude_rules_for_plugins(text: str, names: set[str]) -> str:
    """Drop exclusion rules whose ``linters:`` list is empty after removing plugins.

    Also removes plugin names from multi-linter rules. Rules that still have
    ≥1 other linter (plus path/text/source) are kept.
    """
    if not names:
        return text
    lines = text.splitlines(keepends=True)

    # Find exclusions.rules
    rules_idx = None
    rules_indent = None
    for i, line in enumerate(lines):
        if re.match(r"^(\s+)rules:\s*(?:#.*)?$", line):
            # Prefer the one under exclusions (indent 4 typically)
            rules_idx = i
            rules_indent = indent_width(line)
            break
    if rules_idx is None or rules_indent is None:
        return text

    out = lines[: rules_idx + 1]
    i = rules_idx + 1
    while i < len(lines):
        line = lines[i]
        if line.strip() and not line.lstrip().startswith("#"):
            ind = indent_width(line)
            if ind <= rules_indent and not line.lstrip().startswith("-"):
                break
            # New rule starts with "- " at rules_indent+2
            m = re.match(r"^(\s+)-\s+", line)
            if m and indent_width(line) == rules_indent + 2:
                rule_indent = indent_width(line)
                j = i + 1
                while j < len(lines):
                    el = lines[j]
                    if el.strip() and not el.lstrip().startswith("#"):
                        eind = indent_width(el)
                        # Next rule or end of rules block
                        if eind <= rule_indent and (
                            el.lstrip().startswith("-") or eind <= rules_indent
                        ):
                            break
                    j += 1
                rule_lines = lines[i:j]
                new_rule = _filter_rule_lines(rule_lines, names)
                if new_rule is not None:
                    out.extend(new_rule)
                i = j
                continue
        out.append(line if line.endswith("\n") else line + "\n")
        i += 1
    out.extend(lines[i:])
    return "".join(out)


def _filter_rule_lines(rule_lines: list[str], names: set[str]) -> list[str] | None:
    """Return updated rule lines, or None to drop the rule entirely."""
    # Rewrite linter list entries.
    rewritten: list[str] = []
    i = 0
    linter_names: list[str] = []
    while i < len(rule_lines):
        line = rule_lines[i]
        if re.match(r"^\s+linters:\s*(?:#.*)?$", line) or re.match(
            r"^\s+-\s+linters:\s*(?:#.*)?$", line
        ):
            rewritten.append(line if line.endswith("\n") else line + "\n")
            # Consume nested "- name" entries
            base_ind = indent_width(line)
            i += 1
            while i < len(rule_lines):
                el = rule_lines[i]
                if not el.strip() or el.lstrip().startswith("#"):
                    rewritten.append(el if el.endswith("\n") else el + "\n")
                    i += 1
                    continue
                m = re.match(r"^(\s*)-\s+([A-Za-z0-9_-]+)\s*(?:#.*)?$", el)
                if m and indent_width(el) > base_ind:
                    name = m.group(2)
                    if name not in names:
                        linter_names.append(name)
                        rewritten.append(el if el.endswith("\n") else el + "\n")
                    i += 1
                    continue
                break
            continue
        rewritten.append(line if line.endswith("\n") else line + "\n")
        i += 1

    body = "".join(rewritten)
    # Did this rule originally list only stripped plugins?
    orig_linters = re.findall(
        r"^\s+-\s+([A-Za-z0-9_-]+)\s*(?:#.*)?$",
        "".join(rule_lines),
        flags=re.M,
    )
    # Heuristic: names under a linters: key — approx by collecting from original
    # between linters: and next key.
    orig_plugin_only = False
    orig_body = "".join(rule_lines)
    if "linters:" in orig_body:
        # Extract original linter names under linters:
        orig_names: list[str] = []
        capturing = False
        base = None
        for line in rule_lines:
            if re.search(r"linters:\s*(?:#.*)?$", line):
                capturing = True
                base = indent_width(line)
                continue
            if capturing:
                if line.strip() and not line.lstrip().startswith("#"):
                    if base is not None and indent_width(line) <= base:
                        capturing = False
                    else:
                        m = re.match(r"^\s*-\s+([A-Za-z0-9_-]+)\s*(?:#.*)?$", line)
                        if m:
                            orig_names.append(m.group(1))
                            continue
                        # Non-list content under linters — stop
                        if not line.lstrip().startswith("-"):
                            capturing = False
        if orig_names and all(n in names for n in orig_names):
            orig_plugin_only = True

    if orig_plugin_only:
        return None

    # If linters became empty but other criteria remain, drop to avoid
    # golangci "at least 2 of …" validation errors on a lone path/text.
    if "linters:" in body and not linter_names:
        # Remove the empty linters: key + blank following
        cleaned: list[str] = []
        skip_empty_linters = False
        for line in rewritten:
            if re.search(r"linters:\s*(?:#.*)?$", line):
                skip_empty_linters = True
                continue
            if skip_empty_linters:
                if not line.strip() or line.lstrip().startswith("#"):
                    continue
                if re.match(r"^\s+-\s+", line):
                    continue
                skip_empty_linters = False
            cleaned.append(line)
        body2 = "".join(cleaned)
        has_text = bool(re.search(r"^\s+text:", body2, re.M))
        has_source = bool(re.search(r"^\s+source:", body2, re.M))
        has_path = bool(
            re.search(r"^\s+path(-except)?:", body2, re.M)
        )
        if sum([has_text, has_source, has_path]) < 2:
            return None
        return cleaned

    return rewritten


def patch(text: str) -> str:
    names = find_custom_goplugins(text)
    if names:
        print(
            "compat: stripped goplugin custom linters: " + ", ".join(names),
            file=sys.stderr,
        )
        name_set = set(names)
        text = strip_enable_names(text, name_set)
        text = strip_custom_entries(text, name_set)
        text = strip_exclude_rules_for_plugins(text, name_set)
    return patch_issue_caps(text)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("input")
    ap.add_argument("-o", "--output", required=True)
    args = ap.parse_args(argv)
    src = Path(args.input).read_text(encoding="utf-8", errors="replace")
    Path(args.output).write_text(patch(src), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
