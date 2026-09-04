// Package contextcheck_anonkey pins the key an anonymous function is filed
// under.
//
// `(*ssa.Function).RelString(nil)` — which is the key for both the entry memo
// and the exported fact — qualifies a closure by its parent:
//
//	if f.parent != nil {
//		parent := f.parent.RelString(from)
//		for i, anon := range f.parent.AnonFuncs {
//			if anon == f { return fmt.Sprintf("%s$%d", parent, 1+i) }
//		}
//		return f.name // should never happen
//	}
//
// guff returned the bare `Function.name`, so every `run` method in a package
// shared the key `run$1`. Below there are three, and only the first has a
// context to inherit: with one shared key, whichever of them the walk reaches
// first decides the entry type for all three.
//
// k6's `internal/cmd` has five `run` methods; `(*cmdCloudRun).run`'s closure
// inherited `EntryWithCtx` from a namesake and reported a chain upstream never
// reaches (`cloud_run.go:151`).
package contextcheck_anonkey

import "context"

func withCtx(ctx context.Context) {
	_ = ctx
}

// The chain: `chainTop` is invalid because `chainBottom` takes a context and is
// handed a fresh one.
func chainBottom() { withCtx(context.Background()) }

func chainMiddle() { chainBottom() }

func chainTop() { chainMiddle() }

type hasCtx struct{ ctx context.Context }

// `run$1` here captures a context, so upstream reports its call to `chainTop`.
func (h *hasCtx) run() func() {
	ctx := h.ctx
	return func() {
		_ = ctx
		chainTop()
	}
}

type noCtxA struct{ n int }

// `run$1` here captures no context, so upstream is silent.
func (a *noCtxA) run() func() {
	n := a.n
	return func() {
		_ = n
		chainTop()
	}
}

type noCtxB struct{ s string }

// And again.
func (b *noCtxB) run() func() {
	s := b.s
	return func() {
		_ = s
		chainTop()
	}
}
