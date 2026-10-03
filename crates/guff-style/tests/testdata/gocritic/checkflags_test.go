package gocritic

import "testing"

type cfBig struct{ x [64]int } // 512 bytes

// rangeValCopy / rangeExprCopy skip `func TestXxx(*testing.T)` by default
// (`skipTestFuncs: true`).
func TestCfRange(t *testing.T) {
	var arr [64]int
	for i, v := range arr {
		_, _ = i, v
	}
	for _, v := range []cfBig{} {
		_ = v
	}
}

func cfRangeOutsideTest() {
	var arr [64]int
	for i, v := range arr {
		_, _ = i, v
	}
	for _, v := range []cfBig{} {
		_ = v
	}
}
