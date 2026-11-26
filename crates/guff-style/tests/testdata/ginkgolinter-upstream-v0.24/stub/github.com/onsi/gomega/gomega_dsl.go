package gomega

import (
	"time"
	"github.com/onsi/gomega/internal"
	"github.com/onsi/gomega/types"
)

const GOMEGA_VERSION = "1.42.1"
const nilGomegaPanic = `You are trying to make an assertion, but haven't registered Gomega's fail handler.
If you're using Ginkgo then you probably forgot to put your assertion in an It().
Alternatively, you may have forgotten to register a fail handler with RegisterFailHandler() or RegisterTestingT().
Depending on your vendoring solution you may be inadvertently importing gomega and subpackages (e.g. ghhtp, gexec,...) from different locations.
`

type Gomega = types.Gomega

var Default = Gomega(internal.NewGomega(internal.FetchDefaultDurationBundle()))

func NewGomega(fail types.GomegaFailHandler) Gomega {
	panic("stub")
}

type WithT = internal.Gomega
type GomegaWithT = WithT
type inner interface{ Inner() Gomega }

func internalGomega(g Gomega) *internal.Gomega {
	panic("stub")
}
func NewWithT(t types.GomegaTestingT) *WithT {
	panic("stub")
}

var NewGomegaWithT = NewWithT

func RegisterFailHandler(fail types.GomegaFailHandler) {
	panic("stub")
}
func RegisterFailHandlerWithT(_ types.GomegaTestingT, fail types.GomegaFailHandler) {
	panic("stub")
}
func RegisterTestingT(t types.GomegaTestingT) {
	panic("stub")
}
func InterceptGomegaFailures(f func()) []string {
	panic("stub")
}
func InterceptGomegaFailure(f func()) (err error) {
	panic("stub")
}
func ensureDefaultGomegaIsConfigured() {
	panic("stub")
}
func Ω(actual any, extra ...any) Assertion {
	panic("stub")
}
func Expect(actual any, extra ...any) Assertion {
	panic("stub")
}
func ExpectWithOffset(offset int, actual any, extra ...any) Assertion {
	panic("stub")
}
func Eventually(actualOrCtx any, args ...any) AsyncAssertion {
	panic("stub")
}
func EventuallyWithOffset(offset int, actualOrCtx any, args ...any) AsyncAssertion {
	panic("stub")
}
func Consistently(actualOrCtx any, args ...any) AsyncAssertion {
	panic("stub")
}
func ConsistentlyWithOffset(offset int, actualOrCtx any, args ...any) AsyncAssertion {
	panic("stub")
}

var StopTrying = internal.StopTrying
var TryAgainAfter = internal.TryAgainAfter

type PollingSignalError = internal.PollingSignalError

func SetDefaultEventuallyTimeout(t time.Duration) {
	panic("stub")
}
func SetDefaultEventuallyPollingInterval(t time.Duration) {
	panic("stub")
}
func SetDefaultConsistentlyDuration(t time.Duration) {
	panic("stub")
}
func SetDefaultConsistentlyPollingInterval(t time.Duration) {
	panic("stub")
}
func EnforceDefaultTimeoutsWhenUsingContexts() {
	panic("stub")
}
func DisableDefaultTimeoutsWhenUsingContext() {
	panic("stub")
}

type AsyncAssertion = types.AsyncAssertion
type GomegaAsyncAssertion = types.AsyncAssertion
type Assertion = types.Assertion
type GomegaAssertion = types.Assertion
type OmegaMatcher = types.GomegaMatcher
