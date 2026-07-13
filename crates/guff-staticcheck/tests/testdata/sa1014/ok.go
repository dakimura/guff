package main

import "encoding/json"

func main() {
	var v map[string]any
	var i2 any = &v
	p := &v
	var r any
	var data []byte
	json.Unmarshal(data, &v)
	json.Unmarshal(data, i2)
	json.Unmarshal(data, p)
	json.NewDecoder(r).Decode(&v)
}
