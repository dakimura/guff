module example.com/ginkgolinter

go 1.22.0

require github.com/onsi/ginkgo/v2 v2.0.0
require github.com/onsi/gomega v0.0.0

replace github.com/onsi/ginkgo/v2 => ./ginkgo
replace github.com/onsi/gomega => ./gomega
