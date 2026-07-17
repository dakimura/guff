package arangolint

import (
	"context"
	"fmt"

	"github.com/arangodb/go-driver/v2/arangodb"
)

func beginNil(ctx context.Context, db arangodb.Database) {
	cols := arangodb.TransactionCollections{}
	_, _ = db.BeginTransaction(ctx, cols, nil) // missing AllowImplicit
}

func beginEmptyOpts(ctx context.Context, db arangodb.Database) {
	cols := arangodb.TransactionCollections{}
	_, _ = db.BeginTransaction(ctx, cols, &arangodb.BeginTransactionOptions{}) // missing AllowImplicit
}

func queryConcat(ctx context.Context, db arangodb.Database, key string) {
	_, _ = db.Query(ctx, "FOR d IN coll FILTER d._key == "+key+" RETURN d", nil)
}

func querySprintf(ctx context.Context, db arangodb.Database, key string) {
	_, _ = db.Query(ctx, fmt.Sprintf("FOR d IN coll FILTER d._key == %q RETURN d", key), nil)
}

func queryConcatVar(ctx context.Context, db arangodb.Database, key string) {
	q := "FOR d IN coll FILTER d._key == " + key
	_, _ = db.Query(ctx, q, nil)
}
