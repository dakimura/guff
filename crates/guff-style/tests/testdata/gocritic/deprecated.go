package gocritic

// Which doc comments `deprecatedComment` is shown, and which message each
// malformed notice gets.
//
// The checker is `astwalk.WalkerForDocComment`, and that walk starts at
// `f.Decls`: it visits a `GenDecl`'s doc *and* the doc of every spec inside it,
// plus every `Field` under a `TypeSpec`'s type. guff visited only the two
// outermost, which is seven of Tekton pipeline's thirteen diffs. It also
// visited the *package* doc, which upstream's walk never reaches.
//
// `// FINDING` marks a reported line and names the message; the position is
// the `//` of the offending line, not the declaration. Measured against
// golangci-lint 2.12.2 (go-critic v0.14.3) with **every** checker enabled, as
// the golden case runs it: exactly the 20 marked lines, and nothing else.

import (
	// Text before the notice.
	// Deprecated: an ImportSpec doc. // FINDING paragraph
	"sort"
)

var _ sort.IntSlice

// Text before the notice.
// Deprecated: a GenDecl doc. // FINDING paragraph
var deprGenDecl int

const (
	// Text before the notice.
	// Deprecated: a ValueSpec doc inside a group. // FINDING paragraph
	deprValueSpec = 1
)

type (
	// Text before the notice.
	// Deprecated: a TypeSpec doc inside a group. // FINDING paragraph
	deprTypeSpec int
)

type deprFields struct {
	// Text before the notice.
	// Deprecated: a struct field doc. // FINDING paragraph
	fieldDoc int

	// Deprecated indicates something. // FINDING pattern
	// `deprecated in` matches that prefix under EqualFold, and it is the shape
	// behind pipeline's unused `//nolint:gocritic`.
	patternField int

	nested struct {
		// Text before the notice.
		// Deprecated: a doc on a nested struct's field. // FINDING paragraph
		innerDoc int
	}
}

type deprIface interface {
	// Text before the notice.
	// Deprecated: a doc on an interface method. // FINDING paragraph
	Method()
}

// A notice in its own paragraph is the correct form.
//
// Deprecated: nothing to report here.
func deprSeparated() {}

// Deprecated: alone, with no text before it, is also correct.
func deprAlone() {}

// Short.
// Deprecated: reported. // FINDING paragraph
// A previous line shorter than `Deprecated: ` still counts as text.
func deprShortPrev() {}

// A field of an anonymous struct type is not under a TypeSpec, so the walk
// never reaches it — silent in both tools.
var deprAnonStruct = struct {
	// Text before the notice.
	// Deprecated: not visited.
	f int
}{}

// A documented parameter is a `Field` too, but not one under a TypeSpec's
// type — also silent.
func deprParams(
	// Text before the notice.
	// Deprecated: not visited either.
	x int,
) int {
	return x + deprGenDecl + deprValueSpec + int(deprTypeSpec(0)) + deprFields{}.fieldDoc + deprAnonStruct.f
}

// The five messages, one declaration each.

// DEPRECATED: the casing is wrong. // FINDING casing
func deprCasing() {}

// Deprecated, a comma instead of a colon. // FINDING comma
func deprComma() {}

// Deprecatd: one missing letter. // FINDING typo
func deprTypo() {}

// this type is deprecated, use another. // FINDING pattern
func deprPatternType() {}

// this function is deprecated, use another. // FINDING pattern
func deprPatternFunc() {}

// [[deprecated]] in the C++ spelling. // FINDING pattern
func deprPatternBrackets() {}

// note: deprecated for a while now. // FINDING pattern
func deprPatternNote() {}

// deprecated. use somethingElse. // FINDING pattern
func deprPatternDot() {}

// deprecated! use somethingElse. // FINDING pattern
func deprPatternBang() {}

// deprecated use somethingElse. // FINDING pattern
func deprPatternBare() {}

// Text before the notice.
// DEPRECATED: two problems, one report. // FINDING casing
// Deprecated: the walk returns after the first, so this paragraph violation
// is never reached.
func deprOnlyFirst() {}

var _ deprIface
