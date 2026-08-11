package bytes

// The receivers are pointers, as they are in the real `bytes` package. They
// used to be values here, and S1030's port matched the stub rather than the
// standard library: it looked for `(bytes.Buffer).Bytes` where upstream looks
// for `(*bytes.Buffer).Bytes`, so the check never fired outside this fixture.
type Buffer struct{}

func (b *Buffer) String() string { return "" }
func (b *Buffer) Bytes() []byte  { return []byte{} }
