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
