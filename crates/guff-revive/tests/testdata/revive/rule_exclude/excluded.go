package ruleexclude

// This file is matched by the case's `exclude: ["**/excluded.go"]`, so
// `exported` says nothing about it — while every other rule still does.
type Excluded struct{ N int }

// AlsoExcluded is documented, so `exported` has nothing to say about it. The
// comparison against a literal keeps `bool-literal-in-expr` reporting in this
// file, which is what makes the exclude a *per rule* filter rather than a
// skipped file.
func AlsoExcluded(a bool) bool {
	return a == true
}
