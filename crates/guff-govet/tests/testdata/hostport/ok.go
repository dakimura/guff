package ok

import (
	"fmt"
	"net"
)

func join(host, port string) {
	net.Dial("tcp", net.JoinHostPort(host, port))
}

// Never reaches a dial: upstream only inspects a dial's address argument.
func notDialed(host string, port int) string {
	return fmt.Sprintf("%s:%d", host, port)
}

// A bracketed IPv6 literal is already correct, and is not one of the two
// formats the check recognises.
func bracketed(host string, port int) {
	net.Dial("tcp", fmt.Sprintf("[%s]:%d", host, port))
}
