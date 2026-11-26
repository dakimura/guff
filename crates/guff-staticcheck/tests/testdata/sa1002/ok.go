package main

import "time"

func main() {
	time.Parse("2006-01-02", "2020-01-01")
	time.Parse("2006", "2006")
	// A layout with no std element at all is a literal that parses itself.
	// Upstream is silent here; guff used to report it.
	time.Parse("not-a-layout", "")
	time.Parse("yyyy-mm-dd", "")
	// `_2` and `Z07:00` only parse after SA1002's own substitutions.
	time.Parse("Mon Jan _2 15:04:05 MST 2006", "")
	time.Parse("2006-01-02T15:04:05Z07:00", "")
}
