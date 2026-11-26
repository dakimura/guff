//go:build gofuzz

// This file exists to be *excluded*: `//go:build gofuzz` keeps it out of every
// ordinary build, so `go list` reports it under IgnoredGoFiles and the
// analyzer knows there is package source it cannot see. coredns's
// plugin/forward and plugin/grpc each carry one of these.
package ignoredfiles

func Fuzz(data []byte) int { return 0 }
