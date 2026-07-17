package sql

type DB struct{}

type Rows struct{}

type Stmt struct{}

func (db *DB) Query(query string, args ...any) (*Rows, error) {
	return nil, nil
}

func (db *DB) QueryContext(ctx any, query string, args ...any) (*Rows, error) {
	return nil, nil
}

func (db *DB) Prepare(query string) (*Stmt, error) {
	return nil, nil
}

func (r *Rows) Next() bool { return false }

func (r *Rows) Err() error { return nil }

func (r *Rows) Close() error { return nil }

func (r *Rows) Scan(dest ...any) error { return nil }

func (s *Stmt) Close() error { return nil }

func (s *Stmt) Query(args ...any) (*Rows, error) { return nil, nil }
