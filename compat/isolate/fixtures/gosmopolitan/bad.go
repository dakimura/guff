package p

import "time"

var s = "你好"

func Bad() {
	_ = time.Local
	_ = s
}
