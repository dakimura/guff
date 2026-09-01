package main

import (
	"bytes"
	"net"
)

type otherBytes []byte

func main() {
	// Neither side comes from a net.IP.
	var b1, b2 []byte
	bytes.Equal(b1, b2)

	// Only one side does — upstream needs both.
	bytes.Equal(i1, b1)
	bytes.Equal(b1, i1)
	bytes.Equal([]byte(i1), b2)
	bytes.Equal(b1, []byte(i1))

	// A different named byte slice.
	var o1, o2 otherBytes
	bytes.Equal([]byte(o1), []byte(o2))

	// A plain byte-slice literal.
	bytes.Equal([]byte("a"), []byte("b"))
}

var i1 net.IP
