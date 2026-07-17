package arangodb

import "context"

type TransactionCollections struct {
	Read  []string
	Write []string
}

type BeginTransactionOptions struct {
	AllowImplicit bool
	MaxSize       int
}

type QueryOptions struct {
	Count bool
}

type Cursor interface {
	Close() error
}

type Transaction interface {
	Query(ctx context.Context, query string, opts *QueryOptions) (Cursor, error)
	QueryBatch(ctx context.Context, query string, opts *QueryOptions) (Cursor, error)
	ValidateQuery(ctx context.Context, query string) error
	ExplainQuery(ctx context.Context, query string, opts *QueryOptions) (string, error)
}

type Database interface {
	BeginTransaction(ctx context.Context, cols TransactionCollections, opts *BeginTransactionOptions) (Transaction, error)
	Query(ctx context.Context, query string, opts *QueryOptions) (Cursor, error)
	QueryBatch(ctx context.Context, query string, opts *QueryOptions) (Cursor, error)
	ValidateQuery(ctx context.Context, query string) error
	ExplainQuery(ctx context.Context, query string, opts *QueryOptions) (string, error)
}
