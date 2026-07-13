package main

import "encoding/json"

type Safe struct {
	A int
	B string
}

func main() {
	json.Marshal(Safe{A: 1, B: "ok"})
}
