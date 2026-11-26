// Package identical is the fixture for revive's five `identical-*` rules.
//
// All five compare branches by *printed text*: `astutils.GoFmt` runs
// `printer.Config{Tabwidth: 8}.Fprint` over the node with a fresh, empty
// `token.FileSet`. Two properties follow, and this file pins both.
//
// The print is complete — every token of the branch reaches the string, init
// statements and `else` arms included, so branches that differ anywhere are
// never equal. And it is layout-blind: with no file registered in the file set
// every position resolves to zero, so the printer lays the node out canonically
// and how the source happened to wrap a branch does not matter.
package identical

import "fmt"

func a() error { return nil }
func b() error { return nil }

// Init returns differ only inside the `if` init statement.
func Init(c bool) error {
	if c {
		if err := a(); err != nil {
			return err
		}
	} else {
		if err := b(); err != nil {
			return err
		}
	}
	return nil
}

// InitSame is Init with the same call on both sides: a finding.
func InitSame(c bool) error {
	if c {
		if err := a(); err != nil {
			return err
		}
	} else {
		if err := a(); err != nil {
			return err
		}
	}
	return nil
}

// Else differs only in a nested `else` arm.
func Else(c, d bool) string {
	if c {
		if d {
			return "x"
		} else {
			return "y"
		}
	} else {
		if d {
			return "x"
		} else {
			return "z"
		}
	}
}

// Wrapped writes the same statement across two lines on one side and one on
// the other. The print is layout-blind, so this is a finding.
func Wrapped(c bool) string {
	if c {
		return fmt.Sprintf("%s-%s",
			"a", "b")
	} else {
		return fmt.Sprintf("%s-%s", "a", "b")
	}
}

// Loop branches reach the `for` / `range` / `switch` statement kinds.
func Loop(c bool, xs []int) int {
	total := 0
	if c {
		for i, x := range xs {
			total += i * x
		}
	} else {
		for i, x := range xs {
			total += i * x
		}
	}
	return total
}

// LoopDiffers is Loop with a different operator on one side.
func LoopDiffers(c bool, xs []int) int {
	total := 0
	if c {
		for i, x := range xs {
			total += i * x
		}
	} else {
		for i, x := range xs {
			total += i + x
		}
	}
	return total
}

// Decl branches are two identical declarations — the kind the old renderer
// stringified through `{:?}`, positions and all, so they could never match.
func Decl(c bool) int {
	if c {
		var n int = 1
		return n
	} else {
		var n int = 1
		return n
	}
}

// Switch has two identical tagged branches (identical-switch-branches).
func Switch(n int) string {
	switch n {
	case 1:
		return "one"
	case 2:
		return "one"
	}
	return ""
}

// SwitchCond repeats a case expression (identical-switch-conditions). The rule
// only looks at *untagged* switches — in a tagged one the compiler already
// rejects a duplicate constant case.
func SwitchCond(n int, xs []int) string {
	switch {
	case n > len(xs):
		return "a"
	case n > len(xs):
		return "b"
	}
	return ""
}

// Chain has identical arms in an if/else-if chain.
func Chain(n int) string {
	if n > 10 {
		return "big"
	} else if n > 5 {
		return "big"
	}
	return "small"
}

// ChainCond repeats a condition (identical-ifelseif-conditions).
func ChainCond(n int) string {
	if n > 10 {
		return "big"
	} else if n > 10 {
		return "also big"
	}
	return "small"
}
