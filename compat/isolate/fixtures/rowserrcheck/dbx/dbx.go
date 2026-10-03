// Package dbx hands out a *sql.DB, so its callers need not import database/sql.
package dbx

import "database/sql"

func Open() (*sql.DB, error) { return sql.Open("x", "") }
