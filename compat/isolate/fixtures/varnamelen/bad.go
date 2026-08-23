package p

// varnamelen measures the *distance* a short name has to travel, not its length
// alone: `min-name-length` is 3 but a name used within `max-distance` (5) lines
// of its declaration is fine. `i := 1; _ = i` is therefore silent.
func Bad() int {
	n := 1
	_ = 1
	_ = 2
	_ = 3
	_ = 4
	_ = 5
	_ = 6
	return n
}
