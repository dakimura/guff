package context

type Context interface {
	Done() <-chan struct{}
}

func Background() Context {
	return nil
}

func WithCancel(parent Context) (Context, func()) {
	return parent, func() {}
}
