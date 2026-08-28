// Package requiredoc holds every shape that satisfies require-doc, so the
// case can prove the rule is silent for the right reasons rather than silent
// because it never ran.
//
// Adapted from godoc-lint's own testdata/rule/require_doc/good_all.
package requiredoc

// GD = own godoc, TC = trailing comment, PGD = parent godoc.

// godoc
const SingleSingleFooGD = 0

// godoc
const SingleMultiFooGD, SingleMultiBarGD = 0, 0

const (
	// godoc
	MultiSingleFooGD = 0
)

// godoc
type SingleTFooGD int

type (
	// godoc
	MultiTFooGD int
)

const SingleSingleFooTC = 0 // godoc

const SingleMultiFooTC, SingleMultiBarTC = 0, 0 // godoc

const (
	MultiSingleFooTC = 0 // godoc
)

type SingleTFooTC int // godoc

type (
	MultiTFooTC int // godoc
)

// godoc
const (
	MultiSingleFooPGD = 0
)

// godoc
const (
	MultiMultiFooPGD, MultiMultiBarPGD = 0, 0
)

// godoc
type (
	MultiTFooPGD int
)

// godoc
func FuncFoo() {}

// godoc
type TFoo string

// godoc
func (*TFoo) TFooBar() {}

// A blank identifier names nothing, so it is exempt with no godoc at all.

var _ = 0

// The unexported mirror. Without it the `-unexported` sibling case would only
// measure the reported half and could not tell "documented" from "not looked
// at".

// godoc
const singleSingleFooGD = 0

const (
	// godoc
	multiSingleFooGD = 0
)

// godoc
type singleTFooGD int

const singleSingleFooTC = 0 // godoc

type singleTFooTC int // godoc

// godoc
const (
	multiSingleFooPGD = 0
)

// godoc
type (
	multiTFooPGD int
)

// godoc
func funcFoo() {}

// godoc
type tFoo string

// godoc
func (*tFoo) tFooBar() {}

// godoc
func (*tFoo) TFooBar() {}
