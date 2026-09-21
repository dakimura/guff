// The control: the same declaration in a file nothing generated.
package cgo

func handWritten() {
	var y *int = new(int) // FINDING
	_ = y
}
