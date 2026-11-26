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

// Not "ok" since golangci-lint 2.14.0: recvcheck v0.3.x excludes the
// *decoding* half (UnmarshalText/JSON/YAML/XML/Binary, GobDecode), so the
// value `MarshalJSON` stays and mixes with the pointer `SetData`. Under 2.12.2
// (v0.2.0, the encoding half) `MarshalJSON` was the excluded one and this
// read as pointer-only.
type ValueType struct{}

func (v ValueType) MarshalJSON() ([]byte, error)  { return nil, nil }
func (v *ValueType) UnmarshalJSON(b []byte) error { return nil }
func (v *ValueType) SetData(b []byte)             {}
