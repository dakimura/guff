package testing

type T struct{}

func (t *T) Run(name string, f func(t *T)) bool { return true }

func (t *T) Parallel() {}
