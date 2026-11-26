// Package wrap declares print wrappers that package use calls: their kind
// reaches the caller as an isWrapper fact, the way a module's internal/log
// helpers do.
package wrap

import (
	"fmt"
	"log"
)

func Infof(format string, args ...any) { log.Printf(format, args...) }

// Prefixed changes the format before forwarding it, so it is not a wrapper.
func Prefixed(format string, args ...any) { log.Printf("info: "+format, args...) }

func Errorf(format string, args ...any) error { return fmt.Errorf(format, args...) }

func Info(args ...any) { log.Print(args...) }

type Logger struct{ prefix string }

func (l *Logger) Debugf(format string, args ...any) {
	fmt.Print(l.prefix)
	fmt.Printf(format, args...)
}

type Box[T any] struct{ v T }

func (b Box[T]) Logf(format string, args ...any) { fmt.Printf(format, args...) }

// NotAWrapper takes a format but never forwards it.
func NotAWrapper(format string, args ...any) { _ = format; _ = args }
