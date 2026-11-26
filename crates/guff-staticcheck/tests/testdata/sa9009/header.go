// Licensed under something. This header is the whole point of the fixture:
// SA9009 is a lexical rule, but the production parse runs without
// PARSE_COMMENTS and keeps only *some* comment groups — the leading one
// survives, the rest are dropped. guff gated its source scan on "the file has
// no comments at all", which is true only for a file that starts straight at
// `package`. Every file with a license header therefore went unscanned, and
// beats' `filebeat/input/net/manager.go:40` writes `// go:generate moq …`
// under one.
//
// `bad.go` could not see it: its directive *is* the leading comment group.

package main

// go:generate under a header
type header interface{ M() }

/* go:generate in a block comment */
var blockComment int

	// go:generate indented, so not column 1
var indented int

//go:generate echo fine
var fine int

// go:  spaces after the colon
var spacesAfterColon int

// go:Generate capital letter after the colon
var capital int

// go:
var justGo int

// go:build ignore
var buildTag int
