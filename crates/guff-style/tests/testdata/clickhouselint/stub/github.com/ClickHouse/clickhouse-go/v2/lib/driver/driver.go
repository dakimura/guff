package driver

type Rows interface {
	Next() bool
	Scan(dest ...any) error
	Close() error
	Err() error
}

type Batch interface {
	Append(v ...any) error
	Send() error
	Abort() error
	Close() error
}

type Conn interface {
	Query(ctx any, query string, args ...any) (Rows, error)
	PrepareBatch(ctx any, query string, opts ...any) (Batch, error)
}
