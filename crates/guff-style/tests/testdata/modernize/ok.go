package modernize

import (
	"fmt"
	"maps"
	"slices"
	"strings"
	"sync"
	"unsafe"
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

func cutPrefix(s, pre string) string {
	if after, ok := strings.CutPrefix(s, pre); ok {
		return after
	}
	return s
}

func containsNeedle(s []int, needle int) bool {
	return slices.Contains(s, needle)
}

func copyMap(dst, src map[int]string) {
	maps.Copy(dst, src)
}

func rangeSplit(s string) {
	for part := range strings.SplitSeq(s, ",") {
		_ = part
	}
}

func spawn(wg *sync.WaitGroup) {
	wg.Go(func() {
		_ = 1
	})
}

func alreadyCut(s string) string {
	x, _, _ := strings.Cut(s, ",")
	return x
}

func alreadyAdd(ptr unsafe.Pointer) unsafe.Pointer {
	return unsafe.Add(ptr, 1)
}

type Nested struct {
	Inner struct{ N int } `json:"inner,omitzero"`
}
