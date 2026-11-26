package background

import "context"

func consume(ctx context.Context) { _ = ctx }

// bgOnly builds a context and gives it to nobody. `context.Background()`
// returns a context, so upstream classifies the call as ctx-*out* and skips
// it: the function is clean and its callers hear nothing.
func bgOnly() {
	c := context.Background()
	_ = c
}

// CallsBgOnly has a context of its own and still gets no finding.
func CallsBgOnly(ctx context.Context) {
	consume(ctx)
	bgOnly()
}

// bgToClosure hands the fresh context to a closure, and the MakeClosure
// binding is ctx-typed — that is the finding.
func bgToClosure() func() {
	c := context.Background()

	return func() { consume(c) }
}

// CallsBgToClosure is told to pass its own context down.
func CallsBgToClosure(ctx context.Context) {
	consume(ctx)
	_ = bgToClosure()
}
