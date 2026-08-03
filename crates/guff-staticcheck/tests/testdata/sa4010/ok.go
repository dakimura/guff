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

func returnedAppend() ([]int, error) {
	pairs := make([]int, 0, 10)
	for i := 0; i < 3; i++ {
		switch i {
		case 1:
			pairs, err := otherPairs()
			if err != nil {
				return nil, err
			}
			_ = pairs
		default:
			pairs = append(pairs, i)
		}
	}
	return pairs, nil
}

func otherPairs() ([]int, error) { return nil, nil }

func simpleReturnedAppend() []int {
	s := make([]int, 0, 4)
	s = append(s, 1)
	return s
}
