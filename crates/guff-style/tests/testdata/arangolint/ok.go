package arangolint

import (
	"context"

	"github.com/arangodb/go-driver/v2/arangodb"
)

func beginExplicit(ctx context.Context, db arangodb.Database) {
	cols := arangodb.TransactionCollections{}
	_, _ = db.BeginTransaction(ctx, cols, &arangodb.BeginTransactionOptions{AllowImplicit: false})
}

func beginOptsVar(ctx context.Context, db arangodb.Database) {
	cols := arangodb.TransactionCollections{}
	opts := &arangodb.BeginTransactionOptions{}
	opts.AllowImplicit = true
	_, _ = db.BeginTransaction(ctx, cols, opts)
}

func queryStatic(ctx context.Context, db arangodb.Database) {
	_, _ = db.Query(ctx, "FOR d IN coll RETURN d", nil)
}

func queryBind(ctx context.Context, db arangodb.Database) {
	q := "FOR d IN coll FILTER d._key == @key RETURN d"
	_, _ = db.Query(ctx, q, nil)
}
