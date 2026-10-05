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

// golangci-lint 2.14.0 (recvcheck v0.3.x) excludes the *decoding* half, so a
// pointer `UnmarshalJSON` beside a value method is no longer a mix — dapr's
// `ReminderPeriod`. 2.12.2 (v0.2.0) excluded the encoding half instead, which
// is the opposite answer for both of these types.
type Period struct{ raw string }

func (p Period) String() string              { return p.raw }
func (p *Period) UnmarshalJSON([]byte) error { return nil }

// `MarshalJSON` is not excluded any more, so the value receiver mixes with the
// pointer `Set`: a finding since 2.14.0.
type Encoded struct{ raw string }

func (e Encoded) MarshalJSON() ([]byte, error) { return nil, nil }
func (e *Encoded) Set(v string)                { e.raw = v }

// recvcheck names the type, so a second type mixing receivers is a second
// sentence.
type Mixed struct{ n int }

func (m Mixed) Value() int { return m.n }

func (m *Mixed) Set(n int) { m.n = n }
