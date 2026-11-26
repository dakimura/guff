package main

import (
	"encoding/binary"
	"io"
)

type HasInt struct {
	N int
}

type HasString struct {
	S string
}

func main() {
	var order binary.ByteOrder
	var w io.Writer
	binary.Write(w, order, int(1))
	binary.Write(w, order, HasInt{})
	binary.Write(w, order, HasString{})
}
