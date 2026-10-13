// Package mid is the middle of a chain: its facts exist only because it
// imported dep's.
package mid

import "example.com/dep"

// Exit is no-return because dep.Die is.
func Exit() { dep.Die() }

// Thrice is pure because dep.Add is.
func Thrice(a int) int { return dep.Add(a, dep.Twice(a)) }

// Loud is not pure: dep.Print is not.
func Loud(a int) int { return dep.Print(a) }
