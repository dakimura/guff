//go:build go1.22

// In a go 1.21 module this file alone is at go1.22, and upstream gates
// loopclosure per file (`FileUsesGoVersion`): the capture below is not
// reported. The golden case `govet-loopclosure` puts this file in one package
// with `last.go`, whose captures are reported.
package last

func later(xs []int) {
	for i := range xs {
		go func() { _ = i }()
	}
}
