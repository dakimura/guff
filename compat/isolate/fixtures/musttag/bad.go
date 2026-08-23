package p

import "encoding/json"

// musttag fires on the *call*, not on the type: it walks the marshaller's
// argument. A struct on its own is invisible to it.
type Bad struct {
	Name string
	Age  int
}

func Marshal(b Bad) ([]byte, error) {
	return json.Marshal(b)
}
