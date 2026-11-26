package main

// SA4000 exempts a closed list of `math/rand` functions:
//
//	// We special case functions from the math/rand package. Someone ran into
//	// the following false positive: "rand.Intn(2) - rand.Intn(2), which I
//	// wrote to generate values {-1, 0, 1} with {0.25, 0.5, 0.25} probability."
//
// guff matched the prefix `math/rand.` instead. `math/rand/v2` is a different
// import path, so vitess's `rand.IntN(100) - rand.IntN(100)` was a finding
// upstream does not have — and the `//nolint:staticcheck` over it then read as
// unused. Measured against golangci-lint 2.12.2: only `plainSub` reports.

import (
	one "math/rand"
	v2 "math/rand/v2"
)

func v2Sub() int              { return v2.IntN(100) - v2.IntN(100) }
func v2Uint() uint64          { return v2.Uint64N(9) - v2.Uint64N(9) }
func v2Float() float64        { return v2.Float64() - v2.Float64() }
func v2Method(r *v2.Rand) int { return r.IntN(3) - r.IntN(3) }
func v1Sub() int              { return one.Intn(2) - one.Intn(2) }
func v1Method(r *one.Rand) int {
	return r.Intn(3) - r.Intn(3)
}

func plain() int { return 7 }

// The one finding: not a rand function at all.
func plainSub() int { return plain() - plain() }

func main() {
	_, _, _, _, _, _, _ = v2Sub(), v2Uint(), v2Float(), v2Method(nil), v1Sub(), v1Method(nil), plainSub()
}
