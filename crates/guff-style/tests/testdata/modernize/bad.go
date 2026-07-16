package modernize

import (
	"fmt"
	"sort"
	"strings"
)

func takeAny(x interface{}) {}

func rangeLoop(n int) {
	for i := 0; i < n; i++ {
		_ = i
	}
}

func minMax(a, b int) int {
	var x int
	if a < b {
		x = a
	} else {
		x = b
	}
	return x
}

func appendf(name string) []byte {
	return []byte(fmt.Sprintf("hi %s", name))
}

func sortInts(s []int) {
	sort.Slice(s, func(i, j int) bool { return s[i] < s[j] })
}

func forVar(items []int) {
	for _, v := range items {
		v := v
		_ = v
	}
}

func cutPrefix(s, pre string) string {
	if strings.HasPrefix(s, pre) {
		return strings.TrimPrefix(s, pre)
	}
	return s
}

func containsNeedle(s []int, needle int) bool {
	for _, v := range s {
		if v == needle {
			return true
		}
	}
	return false
}

type Nested struct {
	Inner struct{ N int } `json:"inner,omitempty"`
}
