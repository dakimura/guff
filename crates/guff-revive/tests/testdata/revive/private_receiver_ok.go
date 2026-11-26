// Package privatereceiverok skips exported methods on unexported receivers.
package privatereceiverok

type linearReader struct{}

func (r *linearReader) Read(p []byte) (int, error) { return 0, nil }
func (r *linearReader) Close() error               { return nil }

// Public is documented.
type Public struct{}

// Method is documented.
func (p Public) Method() {}
