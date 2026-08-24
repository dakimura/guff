package p

import (
	"context"

	"github.com/arangodb/go-driver/v2/arangodb"
)

func Bad(ctx context.Context, db arangodb.Database) {
	_, _ = db.BeginTransaction(ctx, arangodb.TransactionCollections{}, nil)
}

// arangolint reports each transaction started without the option, so a second
// call is a second finding — and passing the options is the negative.
func AlsoBad(ctx context.Context, db arangodb.Database) {
	_, _ = db.BeginTransaction(ctx, arangodb.TransactionCollections{
		Read: []string{"a"},
	}, nil)
}
