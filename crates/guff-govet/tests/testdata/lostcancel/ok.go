package p

import "context"

func f() {
	_, cancel := context.WithCancel(context.Background())
	defer cancel()
}
