package main

import (
	"encoding/json"
	"io"
)

func main() {
	var v map[string]any
	var i2 any = &v
	p := &v
	var r io.Reader
	var data []byte
	json.Unmarshal(data, &v)
	json.Unmarshal(data, i2)
	json.Unmarshal(data, p)
	json.NewDecoder(r).Decode(&v)
}
