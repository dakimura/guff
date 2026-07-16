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

// UnmarshalJSON uses a pointer by necessity; built-in exclude keeps this clean.
type ValueType struct{}

func (v ValueType) GetData() []byte { return nil }
func (v ValueType) MarshalJSON() ([]byte, error) { return nil, nil }
func (v *ValueType) UnmarshalJSON(b []byte) error { return nil }
