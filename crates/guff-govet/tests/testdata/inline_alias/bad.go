// Uses of a type alias marked `//go:fix inline` are reported, and the fix
// replaces the use by the alias's right-hand side — rendered with whatever
// prefix the target package has at that position, adding the import when the
// file does not already have one.
package aliasfix

import "cmp"

type Local struct{ N int }

// A directive on the declaration.
//
//go:fix inline
type Ord = cmp.Ordered

// A directive on a spec inside a grouped declaration.
type (
	//go:fix inline
	Grouped = Local

	// No directive: uses of this must stay silent.
	Plain = Local
)

// A generic alias: the fix renders the right-hand side instantiated with the
// type arguments at the use, not the declared one.
//
//go:fix inline
type Pair[K comparable, V any] = map[K]V

// `type A B` is a definition, not an alias. Upstream reports
// `invalid //go:fix inline directive: not a type alias` here; guff reports
// none of the four `gofixdirective` validation diagnostics yet (it also
// silently skips a const whose value is `iota`), so the shape is measured in
// docs/COMPAT-HARDENING.md rather than kept here, where it would be a
// permanent golden diff.
type NotAnAlias Local

func useOrd[T Ord](a, b T) bool { return a < b }

func useGrouped(g Grouped) int { return g.N }

func usePlain(p Plain) int { return p.N }

func usePair(m Pair[string, int]) int { return len(m) }

func useNotAnAlias(n NotAnAlias) int { return n.N }

// The right-hand side of another alias declaration is itself a use.
type Indirect = Ord
