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

// A pointer `UnmarshalJSON` beside a value method: excluded since 2.14.0
// (recvcheck v0.3.x), so not a mix any more. dapr's `ReminderPeriod`.
type Period struct{ raw string }

func (p Period) String() string              { return p.raw }
func (p *Period) UnmarshalJSON([]byte) error { return nil }
