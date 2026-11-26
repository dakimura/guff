package noimport

//go:embed nothing/here
var n = 1

// N pins the `hasEmbed` gate: with no `embed` import the directive is not
// scanned at all, and `go list` reports nothing. (The compiler does.)
func N() int { return n }
