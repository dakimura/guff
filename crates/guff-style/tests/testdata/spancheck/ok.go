package spancheck_ok

import "trace"

func okDefer() {
	_, span := trace.Named("app").Start(nil, "op")
	defer span.End()
}

func okExplicit() {
	_, span := trace.Named("app").Start(nil, "op")
	span.End()
}
