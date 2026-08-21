package context

// Enough of `context` for gosec G118: the analyzer matches
// `context.Background` / `TODO` / `With{Cancel,Timeout,Deadline}` by package
// path and function name, and recognises a context *type* either by name or by
// the four-method interface below — so the method set has to be complete.

import "time"

type Context interface {
	Deadline() (deadline time.Time, ok bool)
	Done() <-chan struct{}
	Err() error
	Value(key any) any
}

type CancelFunc func()

func Background() Context { return nil }
func TODO() Context       { return nil }

func WithCancel(parent Context) (Context, CancelFunc)                         { return nil, nil }
func WithTimeout(parent Context, timeout time.Duration) (Context, CancelFunc) { return nil, nil }
func WithDeadline(parent Context, d time.Time) (Context, CancelFunc)          { return nil, nil }
func WithValue(parent Context, key, val any) Context                          { return nil }
