package sqlx

type DB struct{}

type Rows struct{}

func (db *DB) Query(query string, args ...any) (*Rows, error) {
	return nil, nil
}

func (r *Rows) Next() bool { return false }

func (r *Rows) Err() error { return nil }

func (r *Rows) Close() error { return nil }

func (r *Rows) Scan(dest ...any) error { return nil }
