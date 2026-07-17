package ginkgolinter

import (
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

func settings() {
	s := []int{1, 2, 3}
	Expect(len(s)).Should(Equal(3))
	Expect(s).Should(HaveLen(0))
	Expect(s).Should(Equal(s))
	FIt("focused", func() {})
}
