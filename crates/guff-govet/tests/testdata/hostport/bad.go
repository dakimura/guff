package bad

import (
	"fmt"
	"net"
)

func direct(host string, port int) {
	net.Dial("tcp", fmt.Sprintf("%s:%d", host, port))
}

func viaVar(host, port string) {
	addr := fmt.Sprintf("%s:%s", host, port)
	net.Dial("tcp", addr)
}
