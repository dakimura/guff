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

// The other direction: a []byte built only to reach the `bytes` version of a
// function `strings` already has.
func BytesSide(s, sep string) bool {
	return len(string([]byte(s))) > 0 && sep != ""
}

func BufferString(b []byte) string {
	return string(b)
}
