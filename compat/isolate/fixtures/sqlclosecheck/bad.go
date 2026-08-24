package p

import "database/sql"

// sqlclosecheck has two messages, both reported at an **SSA instruction's**
// position — which go/ssa puts at the call's left parenthesis.

// "Rows/Stmt/NamedStmt was not closed"
func NotClosed(db *sql.DB) error {
	rows, err := db.Query("select 1")
	if err != nil {
		return err
	}
	for rows.Next() {
	}
	return rows.Err()
}

// "Close should use defer" — closed, but not deferred.
func CloseWithoutDefer(db *sql.DB) error {
	rows, err := db.Query("select 1")
	if err != nil {
		return err
	}
	for rows.Next() {
	}
	rows.Close()
	return rows.Err()
}

// A prepared statement is the other target type the first message names.
func StmtNotClosed(db *sql.DB) error {
	stmt, err := db.Prepare("select 1")
	if err != nil {
		return err
	}
	_ = stmt
	return nil
}
