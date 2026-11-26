package context

type Context interface {
	Done() <-chan struct{}
}

func Background() Context  { return nil }
func TODO() Context        { return nil }
func WithValue(parent Context, key, val any) Context { return parent }
func WithCancel(parent Context) (Context, func()) { return parent, func() {} }
