package main

func main() {
	for i := 0; i < 10; i++ {
		_ = i
	}
	// Stepped / assign-form posts must not be flagged (upstream skips non-IncDec).
	for i := 0; i < 10; i += 2 {
		_ = i
	}
	for t := 0; t < 10; t = t + 1 {
		_ = t
	}
}

// The post increments the wrong variable, which is what SA4008 looks for — but
// the body assigns the condition variable, so upstream's IR test (a Phi of a
// Const and a Sigma of itself) fails and neither tool reports. This is gitea's
// escapeStreamer.detectRunes, where `pos` advances by the rune size.
func bodyAdvancesCondVar(data []byte, step int) int {
	var i int
	for pos := 0; pos < len(data); i++ {
		pos += step
	}
	return i
}

// Same, through a closure.
func closureAdvancesCondVar(n, step int) int {
	var i int
	for pos := 0; pos < n; i++ {
		advance := func() { pos += step }
		advance()
	}
	return i
}

// Same, through a pointer handed out of the loop.
func addressEscapes(n int) int {
	var i int
	for pos := 0; pos < n; i++ {
		bump(&pos)
	}
	return i
}

func bump(p *int) { *p += 3 }
