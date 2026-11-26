package gomega

import (
	"time"
	"github.com/google/go-cmp/cmp"
	"github.com/onsi/gomega/types"
)

func Equal(expected any) types.GomegaMatcher {
	panic("stub")
}
func BeEquivalentTo(expected any) types.GomegaMatcher {
	panic("stub")
}
func BeComparableTo(expected any, opts ...cmp.Option) types.GomegaMatcher {
	panic("stub")
}
func BeIdenticalTo(expected any) types.GomegaMatcher {
	panic("stub")
}
func BeNil() types.GomegaMatcher {
	panic("stub")
}
func BeTrue() types.GomegaMatcher {
	panic("stub")
}
func BeFalse() types.GomegaMatcher {
	panic("stub")
}
func BeTrueBecause(format string, args ...any) types.GomegaMatcher {
	panic("stub")
}
func BeFalseBecause(format string, args ...any) types.GomegaMatcher {
	panic("stub")
}
func HaveOccurred() types.GomegaMatcher {
	panic("stub")
}
func Succeed() types.GomegaMatcher {
	panic("stub")
}
func MatchError(expected any, functionErrorDescription ...any) types.GomegaMatcher {
	panic("stub")
}
func MatchErrorStrictly(expected error) types.GomegaMatcher {
	panic("stub")
}
func BeClosed() types.GomegaMatcher {
	panic("stub")
}
func Receive(args ...any) types.GomegaMatcher {
	panic("stub")
}
func BeSent(arg any) types.GomegaMatcher {
	panic("stub")
}
func MatchRegexp(regexp string, args ...any) types.GomegaMatcher {
	panic("stub")
}
func ContainSubstring(substr string, args ...any) types.GomegaMatcher {
	panic("stub")
}
func HavePrefix(prefix string, args ...any) types.GomegaMatcher {
	panic("stub")
}
func HaveSuffix(suffix string, args ...any) types.GomegaMatcher {
	panic("stub")
}
func MatchJSON(json any) types.GomegaMatcher {
	panic("stub")
}
func MatchXML(xml any) types.GomegaMatcher {
	panic("stub")
}
func MatchYAML(yaml any) types.GomegaMatcher {
	panic("stub")
}
func BeEmpty() types.GomegaMatcher {
	panic("stub")
}
func HaveLen(count int) types.GomegaMatcher {
	panic("stub")
}
func HaveCap(count int) types.GomegaMatcher {
	panic("stub")
}
func BeZero() types.GomegaMatcher {
	panic("stub")
}
func ContainElement(element any, result ...any) types.GomegaMatcher {
	panic("stub")
}
func BeElementOf(elements ...any) types.GomegaMatcher {
	panic("stub")
}
func BeKeyOf(element any) types.GomegaMatcher {
	panic("stub")
}
func ConsistOf(elements ...any) types.GomegaMatcher {
	panic("stub")
}
func HaveExactElements(elements ...any) types.GomegaMatcher {
	panic("stub")
}
func ContainElements(elements ...any) types.GomegaMatcher {
	panic("stub")
}
func HaveEach(element any) types.GomegaMatcher {
	panic("stub")
}
func HaveKey(key any) types.GomegaMatcher {
	panic("stub")
}
func HaveKeyWithValue(key any, value any) types.GomegaMatcher {
	panic("stub")
}
func HaveField(field string, expected any) types.GomegaMatcher {
	panic("stub")
}
func HaveExistingField(field string) types.GomegaMatcher {
	panic("stub")
}
func HaveValue(matcher types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func BeNumerically(comparator string, compareTo ...any) types.GomegaMatcher {
	panic("stub")
}
func BeTemporally(comparator string, compareTo time.Time, threshold ...time.Duration) types.GomegaMatcher {
	panic("stub")
}
func BeAssignableToTypeOf(expected any) types.GomegaMatcher {
	panic("stub")
}
func Panic() types.GomegaMatcher {
	panic("stub")
}
func PanicWith(expected any) types.GomegaMatcher {
	panic("stub")
}
func BeAnExistingFile() types.GomegaMatcher {
	panic("stub")
}
func BeARegularFile() types.GomegaMatcher {
	panic("stub")
}
func BeADirectory() types.GomegaMatcher {
	panic("stub")
}
func BeASlice() types.GomegaMatcher {
	panic("stub")
}
func BeAnArray() types.GomegaMatcher {
	panic("stub")
}
func HaveHTTPStatus(expected ...any) types.GomegaMatcher {
	panic("stub")
}
func HaveHTTPHeaderWithValue(header string, value any) types.GomegaMatcher {
	panic("stub")
}
func HaveHTTPBody(expected any) types.GomegaMatcher {
	panic("stub")
}
func And(ms ...types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func SatisfyAll(matchers ...types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func Or(ms ...types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func SatisfyAny(matchers ...types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func Not(matcher types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func WithTransform(transform any, matcher types.GomegaMatcher) types.GomegaMatcher {
	panic("stub")
}
func Satisfy(predicate any) types.GomegaMatcher {
	panic("stub")
}
