package p

import "time"

var s = "你好"

func Bad() {
	_ = time.Local
	_ = s
}

// gosmopolitan says two different things: a script-in-source-literal finding
// that names the script, and `usage of time.Local`.
var korean = "안녕하세요"

var mixed = "hello 你好 world"
