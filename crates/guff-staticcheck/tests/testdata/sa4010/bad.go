package main

func main() {
	s := []int{1}
	_ = append(s, 2)
}

// Vault-style append-only loop: result only feeds Phi / further appends.
type charCount struct {
	r rune
	n int
}
type charCounts []charCount

func unusedAppendLoop(runeCounts map[rune]int) {
	chars := charCounts{}
	for r, count := range runeCounts {
		chars = append(chars, charCount{r, count})
	}
}
