package p

import "fmt"

func intConversionCases() {
	var i int
	_ = fmt.Sprintf("%d", i)

	var i8 int8
	_ = fmt.Sprintf("%d", i8)

	var i64 int64
	_ = fmt.Sprintf("%d", i64)

	var u uint
	_ = fmt.Sprintf("%d", u)

	var u64 uint64
	_ = fmt.Sprintf("%d", u64)
}