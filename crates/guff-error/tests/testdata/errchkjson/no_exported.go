package errchkjsonne

import "encoding/json"

type noExported struct {
	hidden string
}

func marshalNoExported() {
	v := noExported{hidden: "x"}
	_, err := json.Marshal(v)
	_ = err
}
