package unparam

// The body must not look like upstream's `dummyImpl` (a block that immediately
// returns constants), or the parameter is skipped regardless of check-exported.
func Exported(x int) int {
	n := 1
	return n + 1
}
