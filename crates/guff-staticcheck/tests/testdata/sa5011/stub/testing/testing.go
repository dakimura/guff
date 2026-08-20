package testing

// T is the shape SA5011 cares about: `Fatal` is a method on a concrete type in
// package "testing", which is what `ctrlflow` proves never returns.
type T struct{}

func (t *T) Fatal(args ...interface{})                 {}
func (t *T) Fatalf(format string, args ...interface{}) {}
func (t *T) Errorf(format string, args ...interface{}) {}

type TB interface {
	Fatal(args ...interface{})
	Fatalf(format string, args ...interface{})
}
