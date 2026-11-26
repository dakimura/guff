package testing

type T struct{}
type B struct {
	N int
}
type F struct{}

func (t *T) Context() interface{} { return nil }
func (t *T) Run(name string, f func(t *T)) {}
func (b *B) Context() interface{}          { return nil }
func (b *B) Loop() bool                    { return false }
func (b *B) StartTimer()                   {}
func (b *B) StopTimer()                    {}
func (b *B) ResetTimer()                   {}
func (b *B) Run(name string, f func(b *B)) {}
func (f *F) Context() interface{}          { return nil }
