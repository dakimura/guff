package spancheck

import "trace"

func badUnassigned() {
	_, _ = trace.Named("app").Start(nil, "op")
}

func badNoEnd() {
	_, span := trace.Named("app").Start(nil, "op")
	_ = span
}

func badUnderscoreSpan() {
	ctx, _ := trace.Named("app").Start(nil, "op")
	_ = ctx
}
