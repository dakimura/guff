package gosmopolitan

import "time"

var greeting = "你好世界"

func f() {
	_ = "hello 世界"
	_ = time.Local
}

func g() {
	_ = time.UTC
}
