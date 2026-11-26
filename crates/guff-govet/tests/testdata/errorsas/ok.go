package ok

import "errors"

type stringer interface {
	String() string
}

func ok() {
	var err error
	var target stringer
	errors.As(err, &target)
}

// `any` is allowed outright: upstream's carve-out is "a target of any is always
// allowed, since it often indicates a value forwarded from another source". The
// test is `types.Identical(t.Underlying(), anyType)`, so it is structural — a
// named interface whose underlying is empty passes too. thanos wraps
// `errors.As` exactly this way (`pkg/errors/errors.go`).
func okAny(err error, target any) bool { return errors.As(err, target) }

func okIfaceLiteral(err error, target interface{}) bool { return errors.As(err, target) }

type anything interface{}

func okNamedEmptyIface(err error, target anything) bool { return errors.As(err, target) }
