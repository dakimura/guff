// The five gocritic checks whose `settings` entry is a number or a flag.
//
// go-critic reads each from its own `Params` block and falls back to a default:
// `hugeParam.sizeThreshold` 80, `rangeValCopy.sizeThreshold` 128,
// `rangeExprCopy.sizeThreshold` 512, `nestingReduce.bodyWidth` 5,
// `truncateCmp.skipArchDependent` **true**. guff had all five baked in as
// constants, so telegraf's `hugeParam`/`rangeValCopy` at 512 — 290 findings
// golangci-lint does not make — went nowhere.
//
// The struct sizes below sit exactly on the three defaults.
package gocritic

type ccSmall struct{ a, b, c, d, e, f, g, h, i, j int } // 80 bytes

type ccMedium struct{ x [16]int } // 128 bytes

type ccLarge struct{ x [64]int } // 512 bytes

func ccTakesSmall(s ccSmall)   {}
func ccTakesMedium(m ccMedium) {}
func ccTakesLarge(l ccLarge)   {}

func ccRangeSmall(xs []ccSmall) {
	for _, v := range xs {
		_ = v
	}
}

func ccRangeMedium(xs []ccMedium) {
	for _, v := range xs {
		_ = v
	}
}

func ccRangeLarge(xs []ccLarge) {
	for _, v := range xs {
		_ = v
	}
}

// rangeExprCopy needs both a key and a value, an *addressable* operand and an
// array type — a parameter is addressable, a slice is not an array.
func ccRangeExprLarge(xs [64]int) {
	for i, v := range xs {
		_ = i
		_ = v
	}
}

func ccRangeExprSmall(xs [8]int) {
	for i, v := range xs {
		_ = i
		_ = v
	}
}

func ccNestOne(xs []int) {
	for range xs {
		if len(xs) > 0 {
			_ = 1
		}
	}
}

func ccNestFive(xs []int) {
	for range xs {
		if len(xs) > 0 {
			_ = 1
			_ = 2
			_ = 3
			_ = 4
			_ = 5
		}
	}
}

// truncateCmp: comparing a truncated conversion. `int` is architecture
// dependent, so the default `skipArchDependent: true` passes over it.
func ccTruncArch(x int, y int32) bool {
	return int32(x) < y
}

func ccTruncFixed(x int64, y int32) bool {
	return int32(x) < y
}
