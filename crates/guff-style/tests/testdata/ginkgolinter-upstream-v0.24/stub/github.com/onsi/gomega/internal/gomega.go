package internal

import (
	"time"
	"github.com/onsi/gomega/types"
)

type Gomega struct {
	Fail           types.GomegaFailHandler
	THelper        func()
	DurationBundle DurationBundle
}

func NewGomega(bundle DurationBundle) *Gomega {
	panic("stub")
}
func (g *Gomega) IsConfigured() bool {
	panic("stub")
}
func (g *Gomega) ConfigureWithFailHandler(fail types.GomegaFailHandler) *Gomega {
	panic("stub")
}
func (g *Gomega) ConfigureWithT(t types.GomegaTestingT) *Gomega {
	panic("stub")
}
func (g *Gomega) Ω(actual any, extra ...any) types.Assertion {
	panic("stub")
}
func (g *Gomega) Expect(actual any, extra ...any) types.Assertion {
	panic("stub")
}
func (g *Gomega) ExpectWithOffset(offset int, actual any, extra ...any) types.Assertion {
	panic("stub")
}
func (g *Gomega) Eventually(actualOrCtx any, args ...any) types.AsyncAssertion {
	panic("stub")
}
func (g *Gomega) EventuallyWithOffset(offset int, actualOrCtx any, args ...any) types.AsyncAssertion {
	panic("stub")
}
func (g *Gomega) Consistently(actualOrCtx any, args ...any) types.AsyncAssertion {
	panic("stub")
}
func (g *Gomega) ConsistentlyWithOffset(offset int, actualOrCtx any, args ...any) types.AsyncAssertion {
	panic("stub")
}
func (g *Gomega) makeAsyncAssertion(asyncAssertionType AsyncAssertionType, offset int, actualOrCtx any, args ...any) types.AsyncAssertion {
	panic("stub")
}
func (g *Gomega) SetDefaultEventuallyTimeout(t time.Duration) {
	panic("stub")
}
func (g *Gomega) SetDefaultEventuallyPollingInterval(t time.Duration) {
	panic("stub")
}
func (g *Gomega) SetDefaultConsistentlyDuration(t time.Duration) {
	panic("stub")
}
func (g *Gomega) SetDefaultConsistentlyPollingInterval(t time.Duration) {
	panic("stub")
}
func (g *Gomega) EnforceDefaultTimeoutsWhenUsingContexts() {
	panic("stub")
}
func (g *Gomega) DisableDefaultTimeoutsWhenUsingContext() {
	panic("stub")
}
