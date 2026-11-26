// Package g122 is gosec's G122: a filesystem operation inside a
// `filepath.Walk` / `WalkDir` callback, on a path derived from the one the walk
// handed in.
//
// The shapes here are about *which callbacks are found*. Upstream resolves the
// callback argument as an SSA value, so a function named at the call site and a
// local variable holding one are the same thing, while a callback that arrives
// as a call result, a struct field or the caller's own parameter resolves to
// nothing. The findings are deduped by the sink's position, so one callback
// reached from three walks is one finding.
package g122

import (
	"os"
	"path/filepath"
)

// fires — the inline literal, the common case.
func G122Inline(root string) error {
	return filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		_, _ = os.ReadFile(path)

		return nil
	})
}

// fires, once, at the sink inside the callback's own body. authelia's
// `internal/suites/utils.go` passes `fixCoveragePath` this way.
func g122Named(path string, info os.FileInfo, err error) error {
	_, _ = os.ReadFile(path)

	return nil
}

func G122ByName(root string) error { return filepath.Walk(root, g122Named) }

// The same callback again: still one finding, because the sink's position is
// what is deduped.
func G122ByNameTwice(root string) error { return filepath.Walk(root, g122Named) }

// …and through a local. In SSA the register simply *is* the function.
func G122ByLocal(root string) error {
	cb := g122Named

	return filepath.Walk(root, cb)
}

// fires — a callback reached *only* through a local, so this one is not the
// dedup of anything.
func g122ViaLocalOnly(path string, info os.FileInfo, err error) error {
	_, _ = os.ReadFile(path)

	return nil
}

func G122LocalOnly(root string) error {
	cb := g122ViaLocalOnly

	return filepath.Walk(root, cb)
}

// silent — the callback does not touch the walked path.
func g122Clean(path string, info os.FileInfo, err error) error {
	_, _ = os.ReadFile("/etc/hosts")

	return nil
}

func G122NamedClean(root string) error { return filepath.Walk(root, g122Clean) }

// silent — a method value's SSA thunk takes the receiver as parameter 0, and
// the rule only looks at a callback whose first parameter is a string.
type g122Walker struct{}

func (w g122Walker) cb(path string, info os.FileInfo, err error) error {
	_, _ = os.ReadFile(path)

	return nil
}

func G122MethodValue(root string) error {
	var w g122Walker

	return filepath.Walk(root, w.cb)
}

// silent — a callback that arrives as a call result resolves to nothing:
// `ResolveFuncs` has no case for a call.
func g122ViaReturn(path string, info os.FileInfo, err error) error {
	_, _ = os.ReadFile(path)

	return nil
}

func g122Pick() func(string, os.FileInfo, error) error { return g122ViaReturn }

func G122ByReturn(root string) error { return filepath.Walk(root, g122Pick()) }

// silent — and neither does a struct field.
type g122Holder struct {
	fn func(string, os.FileInfo, error) error
}

func g122ViaField(path string, info os.FileInfo, err error) error {
	_, _ = os.ReadFile(path)

	return nil
}

func G122ByField(root string) error {
	h := g122Holder{fn: g122ViaField}

	return filepath.Walk(root, h.fn)
}

// silent — the caller's own parameter is not a function value the rule can
// resolve.
func G122ByParam(root string, cb func(string, os.FileInfo, error) error) error {
	return filepath.Walk(root, cb)
}

// fires — `WalkDir` is the second entry point.
func G122WalkDir(root string) error {
	return filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		_, _ = os.ReadFile(path)

		return nil
	})
}
