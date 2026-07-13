package p

import "context"

func f() {
	_, _ = context.WithCancel(context.Background())
}
