package context

type Context interface{ Done() <-chan struct{} }

type CancelFunc func()

func Background() Context { return nil }
func WithValue(parent Context, key, val any) Context { return parent }
func WithCancel(parent Context) (Context, CancelFunc) { return parent, func() {} }
