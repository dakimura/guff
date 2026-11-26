package internal

import (
	"github.com/onsi/gomega/types"
)

type Assertion struct {
	actuals     []any
	actualIndex int
	vet         vetinari
	offset      int
	g           *Gomega
}
type vetinari func(assertion *Assertion, optionalDescription ...any) bool

func NewAssertion(actualInput any, g *Gomega, offset int, extra ...any) *Assertion {
	panic("stub")
}
func (assertion *Assertion) WithOffset(offset int) types.Assertion {
	panic("stub")
}
func (assertion *Assertion) Error() types.Assertion {
	panic("stub")
}
func (assertion *Assertion) Should(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) ShouldNot(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) To(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) ToNot(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) NotTo(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) buildDescription(optionalDescription ...any) string {
	panic("stub")
}
func (assertion *Assertion) match(matcher types.GomegaMatcher, desiredMatch bool, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) vetActuals(optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *Assertion) vetError(optionalDescription ...any) bool {
	panic("stub")
}
func vetActuals(actuals []any, skipIndex int) (bool, string) {
	panic("stub")
}
