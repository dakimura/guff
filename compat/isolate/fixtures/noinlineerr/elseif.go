package p

// Kept in its own file on purpose. Upstream's fix here makes the file
// unparseable, and golangci-lint gofmts per file — so if this shape shared a
// file with a fixable one, that file's gofmt pass would fail too and the
// *correct* fix would land unformatted (`if  err != nil` at column 0).
// Isolating it keeps bad.go byte-identical between the two tools.
// guff withholds here: upstream inserts at the `if` keyword, which sits after
// `else`, producing `} else err := do()` — not parseable Go.
func ElseIf(a bool) {
	if a {
		return
	} else if err := do(); err != nil {
		panic(err)
	}
}
