// Package unexported feeds start-with-name with both halves of the export
// axis, so the fixture answers differently depending on
// options.start-with-name.include-unexported.
package unexported

// Exported does a thing.
func Exported() {}

// This one does not start with the symbol name.
func AlsoExported() {}

// helper is fine.
func helper() {}

// This one does not start with the symbol name either.
func other() {}

// hidden is a type.
type hidden int

// A wrong opener for a type.
type alsoHidden int

// Not the const name.
const value = 1

// Not the var name.
var mutable = 2

// An exported method on an unexported receiver: godoc never renders it, so
// upstream treats the pair as unexported and only include-unexported reaches
// this line.
func (h hidden) Method() {}

// Both of these are skipped: a multi-name decl carries one doc for several
// symbols, and the blank identifier names nothing.
var first, second = 1, 2

// Not a name at all.
var _ = 3
