package sqlclosecheck

import "database/sql"

var db *sql.DB

func missingClose() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	for rows.Next() {
	}
}

func nonDeferClose() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	for rows.Next() {
	}
	_ = rows.Close()
}

func missingStmtClose() {
	stmt, err := db.Prepare("select 1")
	if err != nil {
		return
	}
	_ = stmt
}

func missingAfterReassign() {
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	defer rows.Close()
	rows, err = db.Query("select 2")
	if err != nil {
		return
	}
	for rows.Next() {
	}
}
