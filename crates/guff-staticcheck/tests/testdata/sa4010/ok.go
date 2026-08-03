package main

func main() {
	s := []int{1}
	s = append(s, 2)
	_ = s
}

type charCount struct {
	r rune
	n int
}
type charCounts []charCount

func usedAppendLoop(runeCounts map[rune]int) int {
	chars := charCounts{}
	for r, count := range runeCounts {
		chars = append(chars, charCount{r, count})
	}
	return len(chars)
}
