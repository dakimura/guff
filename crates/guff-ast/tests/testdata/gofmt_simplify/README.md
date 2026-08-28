# `gofmt -s` test corpus

Copied verbatim from `$GOROOT/src/cmd/gofmt/testdata` (go1.26.5) — every file
there whose directive line is `//gofmt -s`, which is exactly the four the
simplifier has rewrites for.

Copyright The Go Authors. All rights reserved. Use of this source code is
governed by a BSD-style license; see the Go distribution's `LICENSE` file.

| pair | rewrite |
|---|---|
| `composites` | composite literal element type elision, including `&T{}` under `*T` |
| `slices1` | `s[a:len(s)]` → `s[a:]` |
| `ranges` | `for x, _ = range` → `for x = range`, `for _ = range` → `for range` |
| `emptydecl` | `removeEmptyDeclGroups` |

`tests/gofmt_simplify.rs` feeds each `.input` through
`guff::format::source_simplified` and requires the `.golden` byte for byte. It
asserts the file count too, so a pair going missing fails rather than shrinking
the corpus silently.

The other 27 files in upstream's directory drive `-r` (rewrite rules, not
ported — `crate::gofmt` still shells out for those) or `-stdin` fragment mode,
so they are not copied.
