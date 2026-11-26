package errchkjson

import "encoding/json"

func uncheckedSafe() {
	var s string
	_, _ = json.Marshal(s) // blank discard of error (omit-safe default)
}

func unsupportedChan() {
	ch := make(chan int)
	_, err := json.Marshal(ch)
	_ = err
}

func uncheckedFloat() {
	var f float64
	_, _ = json.Marshal(f)
}
