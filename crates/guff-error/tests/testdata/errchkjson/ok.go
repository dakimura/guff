package errchkjsonok

import "encoding/json"

func checkedSafe() {
	var s string
	_, err := json.Marshal(s)
	_ = err
}

func checkedUnsafe() {
	var f float64
	b, err := json.Marshal(f)
	_ = b
	_ = err
}

type safeStruct struct {
	Name string
	N    int
}

func checkedStruct() {
	v := safeStruct{Name: "a", N: 1}
	_, err := json.Marshal(v)
	_ = err
}
