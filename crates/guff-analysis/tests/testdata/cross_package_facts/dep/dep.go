// Package dep declares the functions whose facts the other packages import:
// ctrlflow's "cannot return" and purity's "has no side effects".
package dep

import (
	"fmt"
	"os"
)

// Die never returns: os.Exit is a base case of ctrlflow's induction.
func Die() { os.Exit(1) }

type T struct{}

// Die on a method: the fact needs objectpath's method encoding to cross.
func (T) Die() { os.Exit(2) }

// Fail returns on one path, so it is not no-return.
func Fail(fatal bool) {
	if fatal {
		os.Exit(1)
	}
}

// Add is pure by inference.
func Add(a, b int) int { return a + b }

// Twice is pure because Add is, inside this package.
func Twice(a int) int { return Add(a, a) }

type P struct{ X, Y int }

// Sum is pure: its receiver is a struct of basic fields.
func (p P) Sum() int { return p.X + p.Y }

// Print has a side effect.
func Print(a int) int {
	fmt.Println(a)
	return a
}

// Zero is a stub, which purity never marks.
func Zero(int) int { return 0 }
