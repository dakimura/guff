// Package enumdep is a **separate module**, so a run rooted at the fixture
// never analyses it. That is the whole point: upstream loads dependency syntax
// and so has an enum-members fact for everything it imports, while guff
// imports the package and has none. Put this in the same module and the fact
// exists — the test then passes without measuring anything.
package enumdep

// Kind is declared out of both lexical and value order, so a member list built
// by walking the package scope (which is sorted by name) or by sorting on the
// constant values reads differently from the declaration order upstream uses.
type Kind string

const (
	KindZ Kind = "zeta"
	KindA Kind = "alpha"
	KindM Kind = "mu"
)

// Flag has two names for one value, a blank, and an unexported member.
type Flag int

const (
	FlagOne  Flag = 1
	FlagUno  Flag = 1
	_        Flag = 2
	FlagTwo  Flag = 3
	flagHide Flag = 4
)

// Mostly is exported with mostly unexported members: a switch in another
// package can only be asked for the exported one.
type Mostly int

const (
	MostlyOpen Mostly = iota
	mostlyShut
	mostlyGone
)

// KindAlias is an alias, not a defined type.
type KindAlias = Kind

// Struct is not an enum: no const members.
type Struct struct{ N int }
