package main

import "encoding/json"

func main() {
	var v map[string]any
	var i1 any = v
	var r any
	var data []byte
	json.Unmarshal(data, v)
	json.Unmarshal(data, i1)
	json.NewDecoder(r).Decode(v)
}
