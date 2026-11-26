package p

func f(xs []string) {}

func Bad() string {
	a := "repeated"
	b := "repeated"
	c := "repeated"
	// Nested CompositeLit call args must still count under ignore-calls.
	f([]string{"nested"})
	f([]string{"nested"})
	f([]string{"nested"})
	return a + b + c
}

// goconst counts occurrences and names the string, so a second repeated string
// is a second sentence. Numbers are a separate switch (`numbers: true`).
func AlsoRepeated() {
	x := "another"
	y := "another"
	z := "another"
	_, _, _ = x, y, z
}
