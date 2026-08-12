package testing

// Enough of `testing` for `isUnitTestFunc`, which only asks whether the single
// parameter's type renders as `*testing.T`.

type T struct{}

func (t *T) Run(name string, f func(*T)) bool { return true }

type B struct{}

func (b *B) Run(name string, f func(*B)) bool { return true }
