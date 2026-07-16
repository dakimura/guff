package testing

type T struct{}
type B struct{}
type F struct{}

func (t *T) Context() interface{} { return nil }
func (t *T) Run(name string, f func(t *T)) {}
func (b *B) Context() interface{} { return nil }
func (f *F) Context() interface{} { return nil }
