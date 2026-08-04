package p

import "database/sql"

func Bad(db *sql.DB) error {
	rows, err := db.Query("select 1")
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
	}
	return nil // missing rows.Err()
}
