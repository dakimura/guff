package parallel_support

import (
	"io"
	"net"
	"github.com/onsi/ginkgo/v2/reporters"
)

type RPCServer struct {
	listener net.Listener
	handler  *ServerHandler
}

func newRPCServer(parallelTotal int, reporter reporters.Reporter) (*RPCServer, error) {
	panic("stub")
}
func (server *RPCServer) Start() {
	panic("stub")
}
func (server *RPCServer) Close() {
	panic("stub")
}
func (server *RPCServer) Address() string {
	panic("stub")
}
func (server *RPCServer) GetSuiteDone() chan any {
	panic("stub")
}
func (server *RPCServer) GetOutputDestination() io.Writer {
	panic("stub")
}
func (server *RPCServer) SetOutputDestination(w io.Writer) {
	panic("stub")
}
func (server *RPCServer) RegisterAlive(node int, alive func() bool) {
	panic("stub")
}
