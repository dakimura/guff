package p

func Bad() {
	one := 1
	two := 2
	three := 3
	if three == 3 {
		_ = one
		_ = two
		return
	}
	four := 4
	_ = four
}

// The block leading/trailing whitespace rules were unmeasured until 2026-08-27:
// nothing in this fixture put a blank line just inside a brace, so the two
// report sites whose fix range is *not* their report position were never
// exercised (COMPAT-HARDENING 続き 77).
func BlockLeadingWS() {

	x := 1
	_ = x
}

func BlockTrailingWS() {
	x := 1
	_ = x

}

func BlockBothWS() {

	x := 1
	_ = x

}
