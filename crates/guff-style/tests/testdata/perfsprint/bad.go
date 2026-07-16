package p

import "fmt"

func bad() {
	var s string
	_ = fmt.Sprintf("%s", "hello")
	_ = fmt.Sprintf("%v", s)
	_ = fmt.Sprint(s)
	_ = fmt.Sprintf("hello")
	_ = fmt.Errorf("hello")

	_ = fmt.Sprintf("Hello %s", s)

	var b bool
	_ = fmt.Sprintf("%t", b)
	_ = fmt.Sprint(true)

	var i int
	_ = fmt.Sprintf("%d", i)
	_ = fmt.Sprint(42)

	var i8 int8
	_ = fmt.Sprintf("%d", i8)

	var i64 int64
	_ = fmt.Sprintf("%d", i64)

	var u uint
	_ = fmt.Sprintf("%d", u)

	var bs []byte
	_ = fmt.Sprintf("%x", bs)
}
