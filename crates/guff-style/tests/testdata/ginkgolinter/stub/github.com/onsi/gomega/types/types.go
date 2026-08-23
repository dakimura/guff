// Package types is the shape ginkgolinter looks for before it does anything
// else: `GetGomegaHandler` walks the package's transitive imports for a package
// whose path ends in "github.com/onsi/gomega/types" and reads three interfaces
// out of it. Without them the handler is nil and upstream reports nothing at
// all — which a golden of zero keys cannot be told apart from "no findings".
package types

type GomegaMatcher interface {
	Match(actual interface{}) (success bool, err error)
	FailureMessage(actual interface{}) (message string)
	NegatedFailureMessage(actual interface{}) (message string)
}

type Assertion interface {
	Should(matcher GomegaMatcher, optionalDescription ...interface{}) bool
	ShouldNot(matcher GomegaMatcher, optionalDescription ...interface{}) bool
	To(matcher GomegaMatcher, optionalDescription ...interface{}) bool
	ToNot(matcher GomegaMatcher, optionalDescription ...interface{}) bool
	NotTo(matcher GomegaMatcher, optionalDescription ...interface{}) bool
	WithOffset(offset int) Assertion
}

type AsyncAssertion interface {
	Should(matcher GomegaMatcher, optionalDescription ...interface{}) bool
	ShouldNot(matcher GomegaMatcher, optionalDescription ...interface{}) bool
}
