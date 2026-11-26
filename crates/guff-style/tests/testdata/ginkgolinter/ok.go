package ginkgolinter

import (
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

func ok() {
	s := []int{1, 2, 3}
	var x *int

	Expect(s).To(HaveLen(3))
	Expect(s).To(BeEmpty())
	Expect(x).To(BeNil())
	Expect(true).To(BeTrue())
	Expect(s).To(Equal(s))

	Describe("suite", func() {
		It("works", func() {
			Expect(s).To(HaveLen(3))
		})
	})
}
