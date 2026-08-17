package gosec_g115

// G115 (securego/gosec analyzers/conversion_overflow.go + range_analyzer.go) is
// the second SSA analyzer among the gosec rules guff implements. Almost all of
// the port is the range analysis that decides a conversion is *already* bounded,
// so most of this fixture is no-finding cases: a rule that only compares the two
// types reports every one of them.
//
// Each conversion is marked `// FINDING` or `// silent`, and the golden case
// (compat/golden/cases/gosec) runs both tools over this same file — so the marks
// are checked against golangci-lint 2.12.2, not just asserted here. What gosec
// bounds and what it does not is often surprising (`uint8(u & 0xff)` is silent,
// `uint32(len(b))` is not), which is exactly why they are pinned.
//
// Constants are spelled out rather than taken from `math` so the fixture needs
// no stub beyond `strconv`.

import "strconv"

type MyInt int32

// --- nothing bounds the value ----------------------------------------------

func plain(i int) int32 { return int32(i) } // FINDING int -> int32

func narrowing(i int64) int32 { return int32(i) } // FINDING int64 -> int32

func toUnsigned(i int) uint8 { return uint8(i) } // FINDING int -> uint8

func unsignedToSigned(u uint64) int { return int(u) } // FINDING uint64 -> int

func namedDestination(i int) MyInt { return MyInt(i) } // FINDING int -> int32

func throughPointer(p *int) uint8 { return uint8(*p) } // FINDING int -> uint8

func halfGuarded(i int) int32 {
	// Bounded above only: the low end can still overflow.
	if i > 2147483647 {
		return 0
	}
	return int32(i) // FINDING int -> int32
}

func oneGuardedPredecessor(i int, f bool) uint8 {
	if f {
		if i < 0 || i > 100 {
			return 0
		}
	}
	return uint8(i) // FINDING int -> uint8
}

// --- the destination holds every value the source can: silent --------------

func widening(i int8) int32 { return int32(i) } // silent

func fits(u uint32) int { return int(u) } // silent

// `int`, `uint` and `uintptr` are the same width on every platform, so this
// conversion cannot truncate (gosec `isSameWidthPlatformConversion`).
func platformWord(i int) uint { return uint(i) } // silent

// --- constants -------------------------------------------------------------

func constantInRange() int32 { return int32(1000) } // silent

func constantByte() uint8 { return uint8(255) } // silent

// --- explicit range checks -------------------------------------------------

func guardedBothEnds(i int) int32 {
	if i > 2147483647 || i < -2147483648 {
		return 0
	}
	return int32(i) // silent
}

func guardedInsideIf(i int) int32 {
	if i >= -2147483648 && i <= 2147483647 {
		return int32(i) // silent
	}
	return 0
}

func guardedFlipped(i int) int32 {
	if 2147483647 < i || -2147483648 > i {
		return 0
	}
	return int32(i) // silent
}

func guardedUnsigned(u uint64) uint32 {
	if u > 4294967295 {
		return 0
	}
	return uint32(u) // silent
}

func guardedEarlyReturns(i int) uint8 {
	if i < 0 {
		return 0
	}
	if i > 255 {
		return 0
	}
	return uint8(i) // silent
}

func guardedThenArithmetic(i int) uint8 {
	if i < 0 || i > 100 {
		return 0
	}
	return uint8(i + 1) // silent
}

func guardedEquality(i int) uint8 {
	if i == 7 {
		return uint8(i) // silent
	}
	return 0
}

func guardedThroughPointer(p *int) uint8 {
	if *p < 0 || *p > 255 {
		return 0
	}
	return uint8(*p) // silent
}

// gosec walks the conversion block's predecessors so that an `||`-shaped guard
// still counts (`overflowState.isSafeFromPredecessor`), but it only reads the
// `if` that ends a predecessor or its single parent. Here each arm's guard is
// itself two `if`s, so the edge it finds bounds one side only — a finding.
func bothPredecessorsGuarded(i int, f bool) uint8 {
	if f {
		if i < 0 || i > 100 {
			return 0
		}
	} else {
		if i < 10 || i > 20 {
			return 0
		}
	}
	return uint8(i) // FINDING int -> uint8
}

// --- ranges implied by the operation ---------------------------------------

func masked(u uint64) uint8 { return uint8(u & 0xff) } // silent

func shifted(u uint64) uint32 { return uint32(u >> 32) } // silent

// `len` is non-negative, which bounds the *low* end only: uint32 cannot hold
// every non-negative int, so this is a finding.
func length(b []byte) uint32 { return uint32(len(b)) } // FINDING int -> uint32

// `i % 256` is in [-255, 255] for a signed `i`, and -255 is outside the window
// gosec allows for a uint8 destination.
func remainder(i int) uint8 { return uint8(i % 256) } // FINDING int -> uint8

// `min`/`max` intersect their arguments' bounds by *and*-ing the `set` flags, so
// one unbounded argument leaves the result unbounded.
func minBuiltin(i int) int32 { return int32(min(i, 2147483647)) } // FINDING int -> int32

func maxAndMin(i int) uint8 { return uint8(max(0, min(i, 255))) } // FINDING int -> uint8

// `var acc int` reaches the conversion as a phi whose entry edge is the zero
// constant, and gosec resolves that phi to [0, 0] — the loop edge's own maximum
// is unknown, so it never widens the bound. Reading that entry edge as "no value
// known" instead makes ordinary accumulator code a finding, so it is gated here.
func accumulated(bs []byte) uint8 {
	var acc int
	for _, b := range bs {
		acc += int(b)
	}
	return uint8(acc) // silent
}

// --- strconv bit sizes -----------------------------------------------------

func parsedToWidth(s string) int32 {
	v, err := strconv.ParseInt(s, 10, 32)
	if err != nil {
		return 0
	}
	return int32(v) // silent
}

func parsedTooWide(s string) int32 {
	v, err := strconv.ParseInt(s, 10, 64)
	if err != nil {
		return 0
	}
	return int32(v) // FINDING int64 -> int32
}

func parsedUnsigned(s string) uint16 {
	v, err := strconv.ParseUint(s, 10, 16)
	if err != nil {
		return 0
	}
	return uint16(v) // silent
}
