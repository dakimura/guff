// Package a imports b, which imports a.
package a

import "example.com/cycle/b"

// Name is read by b.
const Name = "a"

// A is never type-checked: the package does not load.
var A = b.B
