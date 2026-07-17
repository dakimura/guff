package rowserrcheck

import "github.com/jmoiron/sqlx"

var xdb *sqlx.DB

func missingSqlxErr() {
	rows, err := xdb.Query("select 1")
	if err != nil {
		return
	}
	for rows.Next() {
	}
}

func withSqlxErr() {
	rows, err := xdb.Query("select 1")
	if err != nil {
		return
	}
	for rows.Next() {
	}
	_ = rows.Err()
}
