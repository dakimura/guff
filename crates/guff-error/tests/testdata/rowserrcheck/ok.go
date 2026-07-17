package rowserrcheck

import "database/sql"

var db *sql.DB

func withErr() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	for rows.Next() {
	}
	_ = rows.Err()
}

func withDeferErr() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	defer func() { _ = rows.Err() }()
	for rows.Next() {
	}
}

func returnsRows() (*sql.Rows, error) {
	return db.Query("select 1")
}

func returnsRowsNamed() (*sql.Rows, error) {
	rows, err := db.Query("select 1")
	if err != nil {
		return nil, err
	}
	return rows, nil
}

func blankRows() {
	_, err := db.Query("select 1")
	if err != nil {
		return
	}
}
