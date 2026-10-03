// Package indirect uses rows it never names: the *sql.DB comes from dbx, and
// this package does not import database/sql itself. Upstream only checks a
// package whose SSA program holds database/sql, which is built from the
// package's own imports, so nothing here is reported — the missing rows.Err
// included.
package indirect

import (
	"context"

	"github.com/dakimura/guff/compat/isolate/fixtures/rowserrcheck/dbx"
)

func Unchecked(ctx context.Context) error {
	db, err := dbx.Open()
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, "select 1")
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
	}
	return nil
}
