package ireturn

import (
	"context"
	"io"
)

// Default allow covers error / empty / anon / stdlib.
func returnsError() error {
	return nil
}

func returnsEmpty() interface{} {
	return 1
}

func returnsAny() any {
	return 1
}

func returnsAnon() interface {
	Do()
} {
	return nil
}

func returnsStdContext() context.Context {
	return context.Background()
}

func returnsStdWriter() io.Writer {
	return nil
}

func returnsConcrete() *int {
	return nil
}
