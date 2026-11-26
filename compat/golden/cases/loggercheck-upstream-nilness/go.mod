module a

go 1.26.0

require (
	github.com/go-logr/logr v0.0.0
	k8s.io/klog/v2 v2.0.0
)

replace github.com/go-logr/logr => ./_stub/github.com/go-logr/logr
replace k8s.io/klog/v2 => ./_stub/k8s.io/klog/v2
