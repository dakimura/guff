package p

import (
	"encoding/json"
	"encoding/xml"
)

// musttag fires on the *call*, not on the type: it walks the marshaller's
// argument. A struct on its own is invisible to it — and each supported
// marshaller is its own entry in upstream's function table, naming its own tag.

type Bad struct {
	Name string
	Age  int
}

func MarshalJSON(b Bad) ([]byte, error) {
	return json.Marshal(b)
}

func UnmarshalJSON(data []byte, b *Bad) error {
	return json.Unmarshal(data, b)
}

func MarshalXML(b Bad) ([]byte, error) {
	return xml.Marshal(b)
}
