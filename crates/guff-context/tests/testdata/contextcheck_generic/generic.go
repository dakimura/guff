// A generic function reached through an instance.
//
// go/ssa (without InstantiateGenerics) builds each instance as a wrapper named
// with its type arguments, `wrap[int]`, whose body calls the origin `wrap`. The
// chain in the message names both. guff keyed the wrapper by the origin's
// object alone, so when a caller reached the instance *before* the origin was
// checked — the callers come first in this file on purpose — the wrapper found
// its own key in the cycle guard, judged itself valid, and overwrote the
// origin's verdict. Every report through it disappeared.
package generic

import "context"

type hookCtx interface {
	context.Context
	Hook()
}

func register(h func(hookCtx) error) error { _ = h; return nil }

func useCtx(context.Context) error { return nil }

type state struct{ n int }

func viaMethod(s *state) error {
	return viaFunc(s)
}

func viaFunc(s *state) error {
	return wrap[map[string]any](func(context.Context, map[string]any) error {
		s.n++
		return nil
	})
}

func viaSecondInstance() error {
	return wrap[int](func(context.Context, int) error { return nil })
}

func Entry(ctx context.Context) error {
	_ = ctx
	if err := viaMethod(&state{}); err != nil {
		return err
	}
	return viaSecondInstance()
}

// The handler's context is the hook's own, converted to `context.Context` —
// not one inherited from a caller — so `wrap` is a function that makes a
// context of its own.
func wrap[T any](run func(context.Context, T) error) error {
	return register(func(ctx hookCtx) error {
		var t T
		return run(ctx, t)
	})
}

// A method of a generic type, called through an instance: upstream reports
// nothing here (the instance has no body to follow), and neither does guff.
type box[T any] struct{ v T }

func (b *box[T]) do() error {
	return register(func(ctx hookCtx) error { return useCtx(ctx) })
}

func viaGenericMethod() error {
	b := &box[int]{}
	return b.do()
}

func EntryMethod(ctx context.Context) error {
	_ = ctx
	return viaGenericMethod()
}
