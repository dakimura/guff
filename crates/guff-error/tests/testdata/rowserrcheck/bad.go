package rowserrcheck

import "database/sql"

var db *sql.DB

func missingErr() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	for rows.Next() {
	}
}

func missingAfterReassign() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	_ = rows.Err()
	rows, err = db.Query("select 2")
	if err != nil {
		return
	}
	for rows.Next() {
	}
}
