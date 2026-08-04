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
	})
})
