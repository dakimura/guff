package context

import "time"

type Context interface {
	Done() <-chan struct{}
}

type CancelFunc func()

func Background() Context {
	return nil
}

func WithCancel(parent Context) (Context, CancelFunc) {
	return parent, func() {}
}

func WithTimeout(parent Context, d time.Duration) (Context, CancelFunc) {
	return parent, func() {}
}

func WithDeadline(parent Context, t time.Time) (Context, CancelFunc) {
	return parent, func() {}
}
