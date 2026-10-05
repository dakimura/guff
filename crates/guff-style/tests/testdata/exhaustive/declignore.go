// Declaration-side directives (exhaustive v0.13.0, golangci-lint 2.14.0):
// `//exhaustive:ignore` in the doc of a type or const declaration (or of one
// spec in a group) drops the type's or the constants' membership, and an
// unparsable directive in any such doc is reported at the start of the doc.
package declignore

// Kind is a plain enum with every member, the control.
type Kind int

const (
	KindA Kind = iota
	KindB
)

// Base is ignored only through its alias below: the constants spelled with
// the alias type are dropped, the ones spelled with Base itself are not.
type Base int

//exhaustive:ignore
type BaseAlias = Base

const (
	BaseA Base      = 1
	BaseB BaseAlias = 2
)

// Ignored is ignored directly; an alias of it does not bring its constants
// back, and constants spelled with the alias are not removed: their type is
// the alias, not Ignored.
//
//exhaustive:ignore
type Ignored int

type IgnoredAlias = Ignored

const (
	IgnoredA Ignored      = 1
	IgnoredB IgnoredAlias = 2
)

// BadType carries a misspelt directive: the diagnostic sits on this line, the
// first of the doc, and the type stays an enum.
//
//exhaustive:ingore
type BadType int

const (
	BadTypeA BadType = 1
	BadTypeB BadType = 2
)

type (
	//exhaustive:ignore-me
	BadSpec int

	//exhaustive:ignore trailing words are fine
	GoodSpecIgnored int
)

const (
	BadSpecA         BadSpec         = 1
	GoodSpecIgnoredA GoodSpecIgnored = 1
)

// Conflicting directives are an error too, and an error never ignores.
//
//exhaustive:ignore
//exhaustive:enforce
type Conflict int

const ConflictA Conflict = 1

// A decl-level ignore skips the specs' docs: nothing is parsed there.
//
//exhaustive:ignore
type (
	//exhaustive:bogus
	SkippedSpec int
)

const SkippedSpecA SkippedSpec = 1

//exhaustive:enfrce
const (
	BadConstA Kind = 10
	//exhaustive:wat
	BadConstB Kind = 11
)

//exhaustive:ignore
const (
	//exhaustive:not-parsed
	IgnoredConst Kind = 12
)

const (
	// Spec-level ignore drops one constant of the group.
	//exhaustive:ignore
	SpecIgnored Kind = 13
	KindC       Kind = 14
)

// A var declaration's doc is not read.
//
//exhaustive:nope
var KindVar Kind = 1

// enforce is a valid directive on a declaration, and changes nothing.
//
//exhaustive:enforce
type Enforced int

const EnforcedA Enforced = 1

func locals() {
	//exhaustive:huh
	type Local int

	//exhaustive:ignore
	const (
		LocalA Local = 1
	)
	const LocalB Local = 2

	var l Local
	switch l {
	}

	//exhaustive:what
	const LocalC Local = 3
	_ = LocalC
}

func switches(k Kind, b Base, i Ignored, bt BadType, bs BadSpec, gs GoodSpecIgnored, c Conflict, ss SkippedSpec, e Enforced) {
	switch k {
	}
	switch b {
	}
	switch i {
	}
	switch bt {
	}
	switch bs {
	}
	switch gs {
	}
	switch c {
	}
	switch ss {
	}
	switch e {
	}
}
