package testing

type T struct{}

func (t *T) Parallel() {}
func (t *T) Setenv(key, value string) {}
func (t *T) Cleanup(f func()) {}
func (t *T) Run(name string, f func(*T)) bool { return true }
func (t *T) Error(args ...any) {}
