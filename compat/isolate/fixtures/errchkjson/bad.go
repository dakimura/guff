package p

import "encoding/json"

func Bad() {
	var f float64
	_, _ = json.Marshal(f) // unsafe type, error discarded
}
