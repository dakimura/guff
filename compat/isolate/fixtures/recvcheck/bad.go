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

// The built-in exclusion list golangci-lint 2.12.2 pins (recvcheck v0.2.0) is
// the *encoding* half, so a pointer `UnmarshalJSON` beside a value method is
// still a mix — dapr's `ReminderPeriod`. v0.3.0 swapped the list for the
// decoding half, which inverts both of these.
type Period struct{ raw string }

func (p Period) String() string              { return p.raw }
func (p *Period) UnmarshalJSON([]byte) error { return nil }

// With `MarshalJSON` excluded there is no value receiver left, so this one is
// not a finding.
type Encoded struct{ raw string }

func (e Encoded) MarshalJSON() ([]byte, error) { return nil, nil }
func (e *Encoded) Set(v string)                { e.raw = v }

// recvcheck names the type, so a second type mixing receivers is a second
// sentence.
type Mixed struct{ n int }

func (m Mixed) Value() int { return m.n }

func (m *Mixed) Set(n int) { m.n = n }
