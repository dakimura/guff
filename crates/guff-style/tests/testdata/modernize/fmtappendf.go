package fmtappendf

import (
	"encoding/json"
	"fmt"
)

type myBytes []byte

type aliasBytes = []byte

// The conversion has to be to `[]byte` itself. `types.Identical` is what
// upstream asks, so a *named* byte slice keeps its own methods and its own
// nil-vs-empty contract and is left alone; an alias still qualifies.
func plain(x int) []byte          { return []byte(fmt.Sprintf("%d", x)) }
func named(x int) json.RawMessage { return json.RawMessage(fmt.Sprintf("%d", x)) }
func localNamed(x int) myBytes    { return myBytes(fmt.Sprintf("%d", x)) }
func alias(x int) aliasBytes      { return aliasBytes(fmt.Sprintf("%d", x)) }
func toString(x int) string       { return string(fmt.Sprintf("%d", x)) }

// `[]byte(fmt.Sprintf(""))` is an empty but non-nil slice while
// `fmt.Appendf(nil, "")` is nil, so `Sprint` and `Sprintf` are skipped whenever
// the format may render empty: the whole string is operations and every verb is
// one of `s v x X`. Any other verb, or one byte of literal text, and the
// rewrite is offered. `Sprintln` always writes a newline and is never skipped.
func fEmpty() []byte               { return []byte(fmt.Sprintf("")) }
func fD(x int) []byte              { return []byte(fmt.Sprintf("%d", x)) }
func fS(x string) []byte           { return []byte(fmt.Sprintf("%s", x)) }
func fV(x any) []byte              { return []byte(fmt.Sprintf("%v", x)) }
func fLowerX(x string) []byte      { return []byte(fmt.Sprintf("%x", x)) }
func fUpperX(x string) []byte      { return []byte(fmt.Sprintf("%X", x)) }
func fQ(x string) []byte           { return []byte(fmt.Sprintf("%q", x)) }
func fLeadingText(x string) []byte { return []byte(fmt.Sprintf("a%s", x)) }
func fTwoS(x, y string) []byte     { return []byte(fmt.Sprintf("%s%s", x, y)) }
func fPrecS(x string) []byte       { return []byte(fmt.Sprintf("%.5s", x)) }
func fPrecD(x int) []byte          { return []byte(fmt.Sprintf("%.5d", x)) }
func fWidthS(x string) []byte      { return []byte(fmt.Sprintf("%10s", x)) }
func fNoVerb() []byte              { return []byte(fmt.Sprintf("plain")) }
func fPercent(x string) []byte     { return []byte(fmt.Sprintf("%%%s", x)) }
func fIndexed(x string) []byte     { return []byte(fmt.Sprintf("%[1]s", x)) }
func fMixed(x int) []byte          { return []byte(fmt.Sprintf("%v%d", x, x)) }

// `Sprint` has no format string of its own; the guard only bites when the first
// argument is a constant.
func sprintVar(x int) []byte   { return []byte(fmt.Sprint(x)) }
func sprintEmpty() []byte      { return []byte(fmt.Sprint("")) }
func sprintNoArgs() []byte     { return []byte(fmt.Sprint()) }
func sprintlnVar(x int) []byte { return []byte(fmt.Sprintln(x)) }
