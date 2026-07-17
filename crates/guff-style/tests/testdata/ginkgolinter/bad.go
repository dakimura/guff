package ginkgolinter

import (
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

func bad() {
	s := []int{1, 2, 3}
	var x *int

	Expect(len(s)).Should(Equal(3))
	Expect(len(s)).Should(BeZero())
	Expect(len(s) == 3).Should(BeTrue())
	Expect(s).Should(HaveLen(0))

	Expect(x).Should(Equal(nil))
	Expect(x == nil).Should(BeTrue())

	Expect(true).Should(Equal(true))

	Expect(s)

	Expect(s).Should(Equal(s)) // force-expect-to only when enabled

	FDescribe("focused", func() {
		FIt("also focused", func() {})
	})
}
