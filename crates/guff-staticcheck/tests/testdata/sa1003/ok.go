package main

import "encoding/binary"

type Payload struct {
	A int32
	B float64
}

func main() {
	var order binary.ByteOrder
	var w any
	binary.Write(w, order, Payload{A: 1, B: 2})
}
