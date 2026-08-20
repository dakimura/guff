package example

type ConsistentPtr struct {
	n int
}

func (c *ConsistentPtr) Inc() { c.n++ }
func (c *ConsistentPtr) Get() int { return c.n }

type ConsistentVal struct {
	n int
}

func (c ConsistentVal) Get() int { return c.n }
func (c ConsistentVal) String() string { return "" }

// The built-in exclusion list golangci-lint 2.12.2 pins (recvcheck v0.2.0) is
// the *encoding* half — MarshalText/JSON/YAML/XML/Binary and GobEncode. With
// `MarshalJSON` excluded the only value receiver here is gone, so the type
// reads as pointer-only and is not a finding. v0.3.0 swapped the list for the
// decoding half, which inverts this.
type ValueType struct{}

func (v ValueType) MarshalJSON() ([]byte, error)  { return nil, nil }
func (v *ValueType) UnmarshalJSON(b []byte) error { return nil }
func (v *ValueType) SetData(b []byte)             {}
