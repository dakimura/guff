// Package limits feeds the `issues.max-issues-per-linter` /
// `issues.max-same-issues` cases a finding set whose *order* is the whole
// point: both limits keep the first N and drop the rest, so which findings
// survive is a statement about the order the runner sees them in.
//
// Everything is in one file of one package on purpose. golangci-lint's
// processors see each linter's findings in whatever order that linter emitted
// them, and for a linter that lints a package's files concurrently (revive
// does) that order is not reproducible across runs — measured: with
// `max-issues-per-linter: 2` on the exclusions fixture, three consecutive runs
// kept different revive findings. Within a single file the emission order is
// the AST walk, which is stable.
//
// The counts are chosen so the two limits disagree about what to drop:
// errcheck reports three findings with one text and then two with another, so
// `max-same-issues: 1` keeps the first and the fourth while
// `max-issues-per-linter: 2` keeps the first and the second.
package limits

import (
	"fmt"
	"os"
)

func mkerr() error { return nil }

// Run reports, in source order: three errcheck findings whose text carries no
// callee name (the call is not a selector), two whose text names `f.Close`,
// and three govet/printf findings that are textually identical to each other.
//
// fmt.Printf is not an errcheck finding: errcheck's own default excluded
// symbols cover the fmt printers.
func Run(f *os.File) {
	mkerr()
	mkerr()
	mkerr()
	f.Close()
	f.Close()
	fmt.Printf("%d\n", "x")
	fmt.Printf("%d\n", "x")
	fmt.Printf("%d\n", "x")
}
