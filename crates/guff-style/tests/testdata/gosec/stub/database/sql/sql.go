package sql

// Enough of database/sql for the G202 fixtures: the receiver *type strings*
// are what gosec's `sqlCallIdents` is keyed by, so `*database/sql.DB` has to be
// spelled the same way here as in the standard library.

type Result interface {
	LastInsertId() (int64, error)
	RowsAffected() (int64, error)
}

type Rows struct{}

func (r *Rows) Close() error { return nil }

type Row struct{}

func (r *Row) Scan(dest ...any) error { return nil }

type DB struct{}

func (db *DB) Exec(query string, args ...any) (Result, error) { return nil, nil }

func (db *DB) Query(query string, args ...any) (*Rows, error) { return nil, nil }

func (db *DB) QueryRow(query string, args ...any) *Row { return nil }

// `Prepare` is in gosec's `sqlCallIdents` at argument 0, same as `Query`.
func (db *DB) Prepare(query string) (*Stmt, error) { return nil, nil }

type Stmt struct{}

type Tx struct{}

func (tx *Tx) Exec(query string, args ...any) (Result, error) { return nil, nil }
