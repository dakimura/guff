// jaeger's `internal/storage/v1/elasticsearch/factory.go`, reduced: a deferred
// closure in a constructor that takes a context calls a Close that calls a
// Close in another package, and the last one passes `context.Background()`.
package outer

import (
	"context"

	"example.com/contextcheck/crosspkg/inner"
)

type Factory struct {
	bulk *inner.Bulk
}

func (f *Factory) Close() error {
	return f.bulk.Close()
}

func New(ctx context.Context) (*Factory, error) {
	f := &Factory{bulk: &inner.Bulk{}}
	ok := false
	defer func() {
		if !ok {
			_ = f.Close()
		}
	}()
	_ = ctx
	ok = true
	return f, nil
}
