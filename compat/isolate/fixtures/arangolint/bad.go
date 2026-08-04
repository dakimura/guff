package p

import (
	"context"

	"github.com/arangodb/go-driver/v2/arangodb"
)

func Bad(ctx context.Context, db arangodb.Database) {
	_, _ = db.BeginTransaction(ctx, arangodb.TransactionCollections{}, nil)
}
