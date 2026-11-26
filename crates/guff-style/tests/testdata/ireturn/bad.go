package ireturn

// Named interfaces — flagged under default allow (empty/error/anon/stdlib).
type Doer interface {
	Do()
}

func NewDoer() Doer {
	return nil
}

type Fooer interface {
	Foo()
}

func NewFooer() Fooer {
	return nil
}
