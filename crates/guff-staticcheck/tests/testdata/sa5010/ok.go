package main

type A interface{ M() }
type B interface{ M() }

func f(a A) { _ = a.(B) }

// The same signature named differently. Upstream asks
// `types.AssignableTo(ml.Type(), mr.Type())`, which for signatures is identity,
// which ignores parameter names — so this assertion is possible. Comparing
// arena ids instead made it "wrong type for Read method: have func(buf []uint8)
// (int, error) want func(p []uint8) (n int, err error)". thanos writes it.
type reader interface {
	Read(p []byte) (n int, err error)
}

type part interface {
	Read(buf []byte) (int, error)
	Size() int64
}

func readerToPart(r reader) part {
	p, _ := r.(part)
	return p
}

// A no-parameter method on both sides: an absent parameter tuple and an empty
// one are the same thing.
type closer interface{ Close() error }

type closerWithName interface {
	Close() (err error)
	Name() string
}

func closerToNamed(c closer) closerWithName {
	n, _ := c.(closerWithName)
	return n
}
