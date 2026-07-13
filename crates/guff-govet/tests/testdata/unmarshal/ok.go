package p

import "encoding/json"

func f() {
	var v int
	json.Unmarshal([]byte("1"), &v)
}
