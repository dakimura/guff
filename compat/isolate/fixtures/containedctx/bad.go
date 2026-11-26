package p

import "context"

type Bad struct {
	ctx context.Context
}

// containedctx walks struct *types*, so an anonymous struct and an embedded
// context are separate nodes reaching the same message.
type Embedded struct {
	context.Context
}

func Anonymous() {
	_ = struct {
		ctx context.Context
	}{}
}
