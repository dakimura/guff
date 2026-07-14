package p

func ok() {
	a := "needconst"
	b := "needconst"
	_ = a
	_ = b
	// Only two occurrences — below golangci default min-occurrences (3).
	x := "ab" // too short for min-len 3
	_ = x
	_ = "ab"
	_ = "ab"
}
