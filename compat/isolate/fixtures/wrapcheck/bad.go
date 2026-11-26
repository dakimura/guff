package p

import "os"

func Bad() error {
	_, err := os.Open("missing")
	return err
}

// A *method* on an external type. Upstream renders the signature with the
// receiver — `func (*os.File).Close() error` — and a port that builds the name
// from the object's package alone says `func os.Close()`, a name Go never
// prints. The fixture above has no method call in it, so that was invisible.
func BadMethod(f *os.File) error {
	return f.Close()
}

func BadMethodAssign(f *os.File) error {
	err := f.Sync()
	return err
}
