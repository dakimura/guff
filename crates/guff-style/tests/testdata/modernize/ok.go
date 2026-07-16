package modernize

import (
	"fmt"
	"slices"
)

func takeAny(x any) {}

func rangeLoop(n int) {
	for i := range n {
		_ = i
	}
}

func minMax(a, b int) int {
	return min(a, b)
}

func appendf(name string) []byte {
	return fmt.Appendf(nil, "hi %s", name)
}

func sortInts(s []int) {
	slices.Sort(s)
}

func forVar(items []int) {
	for _, v := range items {
		_ = v
	}
}

type Nested struct {
	Inner struct{ N int } `json:"inner,omitzero"`
}
