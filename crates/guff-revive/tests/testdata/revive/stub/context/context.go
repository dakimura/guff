package context

type Context interface {
	Done() <-chan struct{}
}

func Background() Context { return nil }

func WithValue(parent Context, key, val interface{}) Context { return parent }
