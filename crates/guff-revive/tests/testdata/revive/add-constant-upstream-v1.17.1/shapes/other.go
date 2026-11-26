package shapes

// String counts are per file: "perfile" appears twice here and twice in
// shapes.go, so the limit of 2 is never exceeded.
func other() {
	use("perfile")
	use("perfile")
	use("thrice", "thrice", "thrice")
}
