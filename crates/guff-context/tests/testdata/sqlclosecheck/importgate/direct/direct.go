// Package direct leaks the same rows as `caller` does and *is* reported,
// because it names database/sql itself. Without it this case's golden would be
// empty, and an empty golden cannot tell "the gate works" from "the linter
// never ran".
package direct

import "database/sql"

func Leaks(db *sql.DB) error {
	rows, err := db.Query("select 1")
	if err != nil {
		return err
	}
	for rows.Next() {
	}
	return nil
}
