package main

// A bare type parameter never subsumes a later case clause.
//
// Upstream's `subsumes` opens with
//
//	if typeparams.IsTypeParam(T) { return false }
//
// and that line is the whole fixture. A type parameter's *underlying* type is
// its constraint's interface, so without the guard `case T:` in a generic
// function looks like an interface everything implements and every later
// clause reads as unreachable. It is not: at any instantiation `T` is one
// concrete type and `*pq[T]` is a different one. grafana/loki's
// `scopeItems[T any]` is that switch, three times over.

import "io"

type pq[T any] struct{ v T }

type reader struct{}

func (reader) Read(b []byte) (int, error) { return 0, nil }

func typeParamFirst[T any](c any) int {
	switch c.(type) {
	case T: // silent: T is a type parameter
		return 1
	case *pq[T]:
		return 2
	default:
		return 0
	}
}

func typeParamSecond[T any](c any) int {
	switch c.(type) {
	case io.Reader: // silent: io.Reader does not subsume a type parameter
		return 1
	case T:
		return 2
	}
	return 0
}

func concreteAfterInterface(c any) int {
	switch c.(type) {
	case io.Reader:
		return 1
	case reader: // FINDING: io.Reader always matches first
		return 2
	}
	return 0
}

func narrowerInterfaceFirst(c any) int {
	switch c.(type) {
	case io.Reader:
		return 1
	case io.ReadCloser: // FINDING: Reader's method set is a subset
		return 2
	}
	return 0
}

func nilIsAlwaysReachable(c any) int {
	switch c.(type) {
	case io.Reader:
		return 1
	case nil: // silent: nil is excluded from the comparison
		return 2
	}
	return 0
}
