// Package lib is dot-imported by both files of the package next door.
package lib

import "errors"

// ErrGone is a sentinel.
var ErrGone = errors.New("gone")

// Hello greets.
func Hello() string { return "hi" }

// Farewell says goodbye.
func Farewell() string { return "bye" }
