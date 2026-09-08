// Package clusterapi carries the three gocritic shapes cluster-api v1.14.0
// turned up: two missed checks and one false positive.
package clusterapi

import (
	"strings"

	"example.com/gocritic/opts"
)

var trueFilter opts.Filter = func(int) bool { return true }

// `dupOption` wants a variadic call whose element type is a func with at least
// one parameter — asked of the type's *underlying*, which guff was not doing,
// so a named func type such as cluster-api's `Func` never qualified.
func dupOptionNamedElem() opts.Filter { return opts.And(trueFilter, trueFilter) }

// reported: an unnamed element type worked even before the fix
func dupOptionAnonElem(f func(int) bool) func(int) bool { return localAnd(f, f) }

func localAnd(filters ...func(int) bool) func(int) bool { return filters[0] }

// silent: the arguments differ
func dupOptionNoDup() opts.Filter {
	return opts.And(trueFilter, func(int) bool { return false })
}

// silent: variadic, but the element is not a func
func dupOptionNotFunc() int { return opts.Sum(3, 3) }

// silent: variadic func element with no parameters is not an option type
func dupOptionZeroParams(t opts.Thunk) { opts.Run(t, t) }

// `offBy1` has four slice forms over strings.Index / bytes.Index that guff did
// not implement; only the `x[len(x)]` index form was there.
func offBy1SliceHigh(segment, delim string) string {
	return segment[:strings.Index(segment, delim)]
}

func offBy1SliceLow(segment, delim string) string {
	return segment[strings.Index(segment, delim):]
}

// silent: the sliced expression is not Index's first argument
func offBy1DifferentSubject(a, b, delim string) string {
	return a[:strings.Index(b, delim)]
}

// `badCond`'s `x == a && x == b` also needs both right-hand sides to be
// side-effect free, and `typep.SideEffectFree` counts a call only when it is a
// type conversion. `len(v)` is a builtin call, so this one is silent.
func badCondLenCalls(v, o []int, i int) int {
	if i == len(v) && i == len(o) {
		return 0
	}
	return 1
}

// reported: plain identifiers on both sides are side-effect free
func badCondIdents(i, a, b int) int {
	if i == a && i == b {
		return 0
	}
	return 1
}

// reported: a conversion counts as side-effect free
func badCondConversion(i int, a, b int32) int {
	if i == int(a) && i == int(b) {
		return 0
	}
	return 1
}
