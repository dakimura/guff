// Package order is about *which* finding `issues.uniq-by-line` keeps when more
// than one linter reports the same line.
//
// The survivor is whichever finding reached the processor first, and nothing
// sorts the slice before `UniqByLine` runs (`SortResults` is the last
// processor of all). Upstream's arrival order is linter **name** order with
// one exception: `nolintlint` is `linter.LastLinter`, so
// `combineGoAnalysisLinters` sorts it behind every other linter regardless of
// name. Every linter in golangci-lint 2.12.2 goes into that one metalinter, so
// that sort is the whole story — `DoesChangeTypes`, which moves `unused` to
// the end of the *top-level* list, never gets to apply.
//
// Each declaration below puts two linters on one line, and the pairs that
// matter are the ones where plain alphabetical order and upstream's order
// disagree. None of the exported functions may carry a doc comment: revive's
// `exported` rule reports *at the comment* when there is one, which would move
// its finding off the line under test and quietly disarm the case.
package order

import "os"

// revive (col 1) vs nolintlint (col 20) on the `Exported` line below.
// Alphabetically `nolintlint` wins; upstream keeps revive, because nolintlint
// is last whatever its name.

func Exported() {} //nolint:errcheck

// unused (col 6) vs nolintlint, the same disagreement — and this line also
// pins that `unused` is *not* moved to the end: it is inside the metalinter,
// so it sorts by name like everything else.

func unusedFn() {} //nolint:errcheck

// The control. gosec (col 2) and errcheck (col 11) both report the unchecked
// call and nolintlint reports the directive, all three on one line, and here
// alphabetical order *is* upstream's order: errcheck wins.
//
// Without the two declarations above, a plain name sort would pass this case.

func Both() {
	os.Setenv("A", "B") //nolint:staticcheck
}
