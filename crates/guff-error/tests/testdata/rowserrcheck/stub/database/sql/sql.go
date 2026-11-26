package sql

type DB struct{}

type Rows struct{}

func (db *DB) Query(query string, args ...any) (*Rows, error) {
	return nil, nil
}

func (db *DB) QueryRow(query string, args ...any) *Row {
	return nil
}

type Row struct{}

func (r *Rows) Next() bool { return false }

func (r *Rows) Err() error { return nil }

func (r *Rows) Close() error { return nil }

func (r *Rows) Scan(dest ...any) error { return nil }
