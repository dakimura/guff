package context

type Context interface{}

func Background() Context { return nil }
func TODO() Context       { return nil }
func WithCancel(parent Context) (ctx Context, cancel func()) {
	return nil, func() {}
}
