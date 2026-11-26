module a

go 1.26.0

require (
	github.com/onsi/ginkgo/v2 v2.0.0
	github.com/onsi/gomega v0.0.0
	github.com/go-logr/logr v0.0.0
	github.com/google/go-cmp v0.0.0
)

replace github.com/onsi/ginkgo/v2 => ./_stub/github.com/onsi/ginkgo/v2
replace github.com/onsi/gomega => ./_stub/github.com/onsi/gomega
replace github.com/go-logr/logr => ./_stub/github.com/go-logr/logr
replace github.com/google/go-cmp => ./_stub/github.com/google/go-cmp
