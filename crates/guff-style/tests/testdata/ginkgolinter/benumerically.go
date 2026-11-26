// Package ginkgolinter's BeNumerically fixture: `Expect(len(s))` against a
// `BeNumerically` matcher, measured against golangci-lint 2.12.2
// (ginkgolinter v0.23.0). The trailing comment on each assertion is what
// upstream suggests, or `silent`.
package ginkgolinter

import . "github.com/onsi/gomega"

const zero = 0

func beNumerically() {
	s := []int{1}
	Expect(len(s)).To(BeNumerically(">=", 0))        // silent: always true
	Expect(len(s)).To(BeNumerically(">", 0))         // ToNot(BeEmpty())
	Expect(len(s)).To(BeNumerically(">=", 1))        // ToNot(BeEmpty())
	Expect(len(s)).To(BeNumerically("!=", 0))        // ToNot(BeEmpty())
	Expect(len(s)).To(BeNumerically("==", 0))        // To(BeEmpty())
	Expect(len(s)).To(BeNumerically(">=", 2))        // silent
	Expect(len(s)).To(BeNumerically(">", 1))         // silent
	Expect(len(s)).To(BeNumerically("<", 1))         // silent
	Expect(len(s)).To(BeNumerically("<=", 0))        // silent
	Expect(len(s)).To(BeNumerically(">", zero))      // ToNot(BeEmpty()): a named constant
	Expect(len(s)).ToNot(BeNumerically(">=", 0))     // silent
	Expect(len(s)).Should(BeNumerically(">", 0))     // ShouldNot(BeEmpty())
	Expect(len(s)).To(BeNumerically("==", 3))        // To(HaveLen(3))
	Expect(len(s)).To(BeNumerically(">=", zero))     // silent
	Expect(len(s)).To(BeNumerically(">=", 1.0))      // silent: 1.0 is not an integer constant
	Expect(len(s)).ToNot(BeNumerically(">", 0))      // To(BeEmpty()): reversed
	Expect(len(s)).ShouldNot(BeNumerically("!=", 0)) // Should(BeEmpty()): reversed
	Expect(len(s)).NotTo(BeNumerically(">", 0))      // To(BeEmpty()): NotTo reverses to To
	Expect(len(s)).To(BeNumerically("!=", zero))     // ToNot(BeEmpty())
	Expect(len(s)).To(BeNumerically("==", zero))     // To(BeEmpty())
	Expect(len(s)).ToNot(BeNumerically(">=", 1))     // To(BeEmpty())
	Expect(len(s)).ToNot(BeNumerically("==", 0))     // ToNot(BeEmpty())
	Expect(len(s)).ToNot(BeNumerically("!=", 3))     // To(HaveLen(3))
	Expect(len(s)).To(BeNumerically("!=", 3))        // ToNot(HaveLen(3))
	Expect(len(s)).To(BeNumerically("==", zero+3))   // To(HaveLen(zero + 3)): the argument as written
	Expect(len(s)).To(BeNumerically(">", 0.0))       // silent
	Expect(len(s)).To(BeNumerically("==", 0.0))      // To(HaveLen(0.0)): 0.0 is not "zero"
}
