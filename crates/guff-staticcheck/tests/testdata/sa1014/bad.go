package main

import (
	"encoding/json"
	"io"
)

func main() {
	var v map[string]any
	var i1 any = v
	var r io.Reader
	var data []byte
	json.Unmarshal(data, v)
	json.Unmarshal(data, i1)
	json.NewDecoder(r).Decode(v)
}
