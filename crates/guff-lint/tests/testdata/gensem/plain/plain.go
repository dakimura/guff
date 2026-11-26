// Package plain is the control: no marker anywhere, so its finding survives
// every mode.
package plain

func mkerr() error { return nil }

// Run has one unchecked error.
func Run() {
	mkerr()
}
