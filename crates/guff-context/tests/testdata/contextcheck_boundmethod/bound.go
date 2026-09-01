package boundmethod

import "context"

func consume(ctx context.Context) { _ = ctx }

type Doc struct{ Text string }

type Completer struct{ n int }

// autoComplete needs a context that its caller does not have.
func (c *Completer) autoComplete(w string) []string {
	consume(context.Background())

	return []string{w}
}

// Complete is the signature a prompt library wants, so it cannot take a
// context — the fix would be to give the Completer one.
func (c *Completer) Complete(d Doc) []string {
	return c.autoComplete(d.Text)
}

func install(f func(Doc) []string) { _ = f }

func installExpr(f func(*Completer, Doc) []string) { _ = f }

// BoundValue hands the method over as a value. go/ssa builds a separate
// `$bound` closure target for that, and its RelString keeps the suffix — so
// the method's own verdict is never looked up and nothing is reported.
func BoundValue(ctx context.Context) {
	consume(ctx)
	c := &Completer{}
	install(c.Complete)
}

// MethodExpression is the `$thunk` form, and is likewise not followed.
func MethodExpression(ctx context.Context) {
	consume(ctx)
	installExpr((*Completer).Complete)
}

// DirectCall is the shape that *is* reported: a plain call to the method.
func DirectCall(ctx context.Context) {
	consume(ctx)
	c := &Completer{}
	_ = c.Complete(Doc{})
}
