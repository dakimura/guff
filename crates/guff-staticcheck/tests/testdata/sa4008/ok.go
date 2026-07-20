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
