package types

import (
	"context"
	"time"
)

type GomegaFailHandler func(message string, callerSkip ...int)
type GomegaTestingT interface {
	Helper()
	Fatalf(format string, args ...any)
}
type Gomega interface {
	Ω(actual any, extra ...any) Assertion
	Expect(actual any, extra ...any) Assertion
	ExpectWithOffset(offset int, actual any, extra ...any) Assertion
	Eventually(actualOrCtx any, args ...any) AsyncAssertion
	EventuallyWithOffset(offset int, actualOrCtx any, args ...any) AsyncAssertion
	Consistently(actualOrCtx any, args ...any) AsyncAssertion
	ConsistentlyWithOffset(offset int, actualOrCtx any, args ...any) AsyncAssertion
	SetDefaultEventuallyTimeout(time.Duration)
	SetDefaultEventuallyPollingInterval(time.Duration)
	SetDefaultConsistentlyDuration(time.Duration)
	SetDefaultConsistentlyPollingInterval(time.Duration)
	EnforceDefaultTimeoutsWhenUsingContexts()
	DisableDefaultTimeoutsWhenUsingContext()
}
type GomegaMatcher interface {
	Match(actual any) (success bool, err error)
	FailureMessage(actual any) (message string)
	NegatedFailureMessage(actual any) (message string)
}
type OracleMatcher interface{ MatchMayChangeInTheFuture(actual any) bool }

func MatchMayChangeInTheFuture(matcher GomegaMatcher, value any) bool {
	panic("stub")
}

type AsyncAssertion interface {
	Should(matcher GomegaMatcher, optionalDescription ...any) bool
	ShouldNot(matcher GomegaMatcher, optionalDescription ...any) bool
	To(matcher GomegaMatcher, optionalDescription ...any) bool
	ToNot(matcher GomegaMatcher, optionalDescription ...any) bool
	NotTo(matcher GomegaMatcher, optionalDescription ...any) bool
	WithOffset(offset int) AsyncAssertion
	WithTimeout(interval time.Duration) AsyncAssertion
	WithPolling(interval time.Duration) AsyncAssertion
	Within(timeout time.Duration) AsyncAssertion
	ProbeEvery(interval time.Duration) AsyncAssertion
	WithContext(ctx context.Context) AsyncAssertion
	WithArguments(argsToForward ...any) AsyncAssertion
	MustPassRepeatedly(count int) AsyncAssertion
}
type Assertion interface {
	Should(matcher GomegaMatcher, optionalDescription ...any) bool
	ShouldNot(matcher GomegaMatcher, optionalDescription ...any) bool
	To(matcher GomegaMatcher, optionalDescription ...any) bool
	ToNot(matcher GomegaMatcher, optionalDescription ...any) bool
	NotTo(matcher GomegaMatcher, optionalDescription ...any) bool
	WithOffset(offset int) Assertion
	Error() Assertion
}
