package gomega

import "github.com/onsi/gomega/types"

// Real gomega aliases these to the `types` package, and ginkgolinter depends on
// that: it resolves `types.Assertion` / `types.AsyncAssertion` /
// `types.GomegaMatcher` and asks whether the expression's type implements them.
// A local `interface{}` would leave every assertion here unrecognised.
type OmegaMatcher = types.GomegaMatcher

type Assertion = types.Assertion

type AsyncAssertion = types.AsyncAssertion

func Expect(actual interface{}, extra ...interface{}) Assertion { return nil }
func ExpectWithOffset(offset int, actual interface{}, extra ...interface{}) Assertion {
	return nil
}
func Eventually(actual interface{}, intervals ...interface{}) AsyncAssertion {
	return nil
}
func Consistently(actual interface{}, intervals ...interface{}) AsyncAssertion {
	return nil
}



func Equal(expected interface{}) OmegaMatcher                         { return nil }
func BeNil() OmegaMatcher                                             { return nil }
func BeTrue() OmegaMatcher                                            { return nil }
func BeFalse() OmegaMatcher                                           { return nil }
func BeZero() OmegaMatcher                                            { return nil }
func BeEmpty() OmegaMatcher                                           { return nil }
func HaveLen(count interface{}) OmegaMatcher                          { return nil }
func BeNumerically(comparator string, compareTo ...interface{}) OmegaMatcher {
	return nil
}
func Not(matcher OmegaMatcher) OmegaMatcher { return nil }
func HaveOccurred() OmegaMatcher            { return nil }
func Succeed() OmegaMatcher                 { return nil }
