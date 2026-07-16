package errchkjsoncef

import "encoding/json"

func checkedSafeShouldWarn() {
	var s string
	_, err := json.Marshal(s)
	_ = err
}
