// Package a imports b; only b's in-package test closes the cycle.
package a

import "example.com/cycle/b"

// A builds on its own.
func A() int { return b.B() }
