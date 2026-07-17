package sqlclosecheck

import "database/sql"

var db *sql.DB

func withDeferClose() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	defer rows.Close()
	for rows.Next() {
	}
}

func withDeferBlockClose() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	defer func() {
		_ = rows.Close()
	}()
	for rows.Next() {
	}
}

func withStmtDefer() {
	stmt, err := db.Prepare("select 1")
	if err != nil {
		return
	}
	defer stmt.Close()
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

func passedToHelper() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	closeRows(rows)
}

func closeRows(rows *sql.Rows) {
	_ = rows.Close()
}

func blankRows() {
	_, err := db.Query("select 1")
	if err != nil {
		return
	}
}
