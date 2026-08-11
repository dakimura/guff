// Package analyzer separates the linter name from the analyzer name. A finding
// carries the *linter* it came from (`govet`), so `//nolint:printf` names
// something the linter registry does not know: it suppresses nothing, and the
// unused candidate it would produce is dropped because `printf` is not an
// enabled linter.
package analyzer

import "fmt"

func ByLinter() {
	fmt.Printf("%d\n", "not a number") //nolint:govet
}

func ByAnalyzer() {
	fmt.Printf("%d\n", "not a number") //nolint:printf
}
