// Package driver hands out a *sql.DB without its caller ever naming
// database/sql — vitess's `vitessdriver.Open` in miniature.
package driver

import "database/sql"

func Open() *sql.DB {
	db, _ := sql.Open("x", "y")
	return db
}
