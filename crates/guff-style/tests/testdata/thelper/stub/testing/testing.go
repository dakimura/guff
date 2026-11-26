package testing

type T struct{}
type B struct{}
type F struct{}
type TB interface {
	Helper()
	Run(name string, f func(*T)) bool
}

func (t *T) Helper()              {}
func (t *T) Fail()                {}
func (t *T) Run(name string, f func(*T)) bool { return true }
func (t *T) Parallel()            {}
func (t *T) Error(args ...any)    {}

func (b *B) Helper()              {}
func (b *B) Run(name string, f func(*B)) bool { return true }

func (f *F) Helper()       {}
func (f *F) Fuzz(ff func(*T)) {}
