package gomega

type OmegaMatcher interface{}

func Expect(actual interface{}, extra ...interface{}) Assertion { return Assertion{} }
func ExpectWithOffset(offset int, actual interface{}, extra ...interface{}) Assertion {
	return Assertion{}
}
func Eventually(actual interface{}, intervals ...interface{}) AsyncAssertion {
	return AsyncAssertion{}
}
func Consistently(actual interface{}, intervals ...interface{}) AsyncAssertion {
	return AsyncAssertion{}
}

type Assertion struct{}

func (Assertion) To(matcher OmegaMatcher, optionalDescription ...interface{}) bool   { return true }
func (Assertion) ToNot(matcher OmegaMatcher, optionalDescription ...interface{}) bool { return true }
func (Assertion) NotTo(matcher OmegaMatcher, optionalDescription ...interface{}) bool { return true }
func (Assertion) Should(matcher OmegaMatcher, optionalDescription ...interface{}) bool {
	return true
}
func (Assertion) ShouldNot(matcher OmegaMatcher, optionalDescription ...interface{}) bool {
	return true
}
func (Assertion) WithOffset(offset int) Assertion { return Assertion{} }

type AsyncAssertion struct{}

func (AsyncAssertion) Should(matcher OmegaMatcher, optionalDescription ...interface{}) bool {
	return true
}
func (AsyncAssertion) ShouldNot(matcher OmegaMatcher, optionalDescription ...interface{}) bool {
	return true
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
