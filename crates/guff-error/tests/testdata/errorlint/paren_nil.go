package errorlint

// Upstream's `isNil` is a bare `ex.(*ast.Ident)` type assertion with no
// unparenthesizing, so a parenthesized nil is not recognized as a nil
// comparison and the check reports it like any other error comparison.
// `err != nil` on the next function is the control: it must stay silent, or a
// fix that simply reports everything would pass.
func parenNil(err error) {
	if err != (nil) {
		_ = err
	}
}

func bareNil(err error) {
	if err != nil {
		_ = err
	}
}
