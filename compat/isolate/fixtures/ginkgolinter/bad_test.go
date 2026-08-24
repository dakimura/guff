package ginkgolinter_test

import (
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

var _ = Describe("x", func() {
	It("bad", func() {
		s := []int{1}
		var x *int
		Expect(len(s)).Should(Equal(1))
		Expect(len(s)).Should(BeZero())
		Expect(s).Should(HaveLen(0))
		Expect(x).Should(Equal(nil))
		Expect(true).Should(Equal(true))
		Expect(s)

		// The subject is rendered into the message with `GoFmtFormatter`
		// (`printer.Fprint`), so a subject whose spelling the two renderers
		// disagree about is what makes that verifiable: go/printer drops the
		// blanks around an operator nested under a lower-precedence one, while
		// an approximation puts blanks around every operator.
		t := []int{1, 2, 3}
		u := []int{4}
		Expect(len(t[len(u)/2+1:])).Should(Equal(1))
		Expect(t[len(u)-1 : len(t)]).Should(HaveLen(0))
	})
})
