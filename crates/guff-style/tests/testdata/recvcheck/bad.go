package example

type RPC struct {
	result int
	done   chan struct{}
}

func (rpc *RPC) compute() {
	rpc.result = 42
	close(rpc.done)
}

func (RPC) version() int {
	return 1
}
