package p

import "io"

func Bad() {
	io.EOF = nil
}

// reassign names the variable, so each package-level target is its own
// sentence. `+=` and friends are assignments too.
func More() {
	io.ErrUnexpectedEOF = nil
	io.Discard = nil
}
