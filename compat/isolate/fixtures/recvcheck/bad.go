package p

type RPC struct {
	result int
	done   chan struct{}
}

func (rpc *RPC) compute() {
	rpc.result = 42
	close(rpc.done)
}

func (rpc RPC) Result() int { // value receiver inconsistent
	return rpc.result
}
