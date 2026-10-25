package internal

import (
	"context"
	"reflect"
	"time"
	"github.com/onsi/gomega/types"
)

var errInterface = reflect.TypeOf((*error)(nil)).Elem()
var gomegaType = reflect.TypeOf((*types.Gomega)(nil)).Elem()
var contextType = reflect.TypeOf(new(context.Context)).Elem()

type formattedGomegaError interface{ FormattedGomegaError() string }
type asyncPolledActualError struct{ message string }

func (err *asyncPolledActualError) Error() string {
	panic("stub")
}
func (err *asyncPolledActualError) FormattedGomegaError() string {
	panic("stub")
}

type contextWithAttachProgressReporter interface{ AttachProgressReporter(func() string) func() }
type asyncGomegaHaltExecutionError struct{}

func (a asyncGomegaHaltExecutionError) GinkgoRecoverShouldIgnoreThisPanic() {
	panic("stub")
}
func (a asyncGomegaHaltExecutionError) Error() string {
	panic("stub")
}

type AsyncAssertionType uint

const (
	AsyncAssertionTypeEventually AsyncAssertionType = iota
	AsyncAssertionTypeConsistently
)

func (at AsyncAssertionType) String() string {
	panic("stub")
}

type AsyncAssertion struct {
	asyncType          AsyncAssertionType
	actualIsFunc       bool
	actual             any
	argsToForward      []any
	timeoutInterval    time.Duration
	pollingInterval    time.Duration
	mustPassRepeatedly int
	ctx                context.Context
	offset             int
	g                  *Gomega
}

func NewAsyncAssertion(asyncType AsyncAssertionType, actualInput any, g *Gomega, timeoutInterval time.Duration, pollingInterval time.Duration, mustPassRepeatedly int, ctx context.Context, offset int) *AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) WithOffset(offset int) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) WithTimeout(interval time.Duration) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) WithPolling(interval time.Duration) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) Within(timeout time.Duration) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) ProbeEvery(interval time.Duration) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) WithContext(ctx context.Context) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) WithArguments(argsToForward ...any) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) MustPassRepeatedly(count int) types.AsyncAssertion {
	panic("stub")
}
func (assertion *AsyncAssertion) Should(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *AsyncAssertion) To(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *AsyncAssertion) ShouldNot(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *AsyncAssertion) ToNot(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *AsyncAssertion) NotTo(matcher types.GomegaMatcher, optionalDescription ...any) bool {
	panic("stub")
}
func (assertion *AsyncAssertion) buildDescription(optionalDescription ...any) string {
	panic("stub")
}
func (assertion *AsyncAssertion) processReturnValues(values []reflect.Value) (any, error) {
	panic("stub")
}
func (assertion *AsyncAssertion) invalidFunctionError(t reflect.Type) error {
	panic("stub")
}
func (assertion *AsyncAssertion) noConfiguredContextForFunctionError() error {
	panic("stub")
}
func (assertion *AsyncAssertion) argumentMismatchError(t reflect.Type, numProvided int) error {
	panic("stub")
}
func (assertion *AsyncAssertion) invalidMustPassRepeatedlyError(reason string) error {
	panic("stub")
}
func (assertion *AsyncAssertion) buildActualPoller() (func() (any, error), error) {
	panic("stub")
}
func (assertion *AsyncAssertion) afterTimeout() <-chan time.Time {
	panic("stub")
}
func (assertion *AsyncAssertion) afterPolling() <-chan time.Time {
	panic("stub")
}
func (assertion *AsyncAssertion) matcherSaysStopTrying(matcher types.GomegaMatcher, value any) bool {
	panic("stub")
}
func (assertion *AsyncAssertion) pollMatcher(matcher types.GomegaMatcher, value any) (matches bool, err error) {
	panic("stub")
}
func (assertion *AsyncAssertion) match(matcher types.GomegaMatcher, desiredMatch bool, optionalDescription ...any) bool {
	panic("stub")
}
