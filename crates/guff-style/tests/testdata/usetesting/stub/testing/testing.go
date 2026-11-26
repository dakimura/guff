package testing

type T struct{}
type B struct{}
type TB interface {
	TempDir() string
}

func (t *T) TempDir() string { return "" }
func (t *T) Log(args ...any) {}
func (b *B) TempDir() string { return "" }
