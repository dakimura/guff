package p

import "strings"

// mirror pairs the string and []byte APIs. `strings.Contains` lists both of its
// arguments in `Args`, and it only fires when **every** listed argument is a
// `string(…)` conversion — one literal on either side and the call is left
// alone, which is why a fixture with `strings.Contains(string(b), "x")` reports
// nothing at all.
func Bad(b, sep []byte) bool {
	return strings.Contains(string(b), string(sep))
}
