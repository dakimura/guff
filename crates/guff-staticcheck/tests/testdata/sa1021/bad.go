package main

import (
	"bytes"
	"net"
)

type myIP = net.IP

func main() {
	// Passing a net.IP straight into bytes.Equal still converts: the
	// parameter is []byte, so the IR holds a ChangeType either way, and
	// upstream's `isConvertedFrom` matches it.
	var i1, i2 net.IP
	bytes.Equal(i1, i2)

	// The conversion written out.
	bytes.Equal([]byte(i1), []byte(i2))

	// Through an alias of net.IP.
	var a1, a2 myIP
	bytes.Equal([]byte(a1), []byte(a2))

	// A conversion of a conversion: the outer one is the ChangeType the
	// check sees, and its operand is still a net.IP.
	bytes.Equal([]byte([]byte(i1)), []byte(i2))
}
