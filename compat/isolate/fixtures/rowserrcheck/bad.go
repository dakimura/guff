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

// rowserrcheck follows the rows value, so a second unchecked query is a second
// finding — and `QueryContext` is the same shape through a different method.
func AlsoBad(db *sql.DB) error {
	rows, err := db.Query("select 2")
	if err != nil {
		return err
	}
	defer rows.Close()

	for rows.Next() {
	}

	return nil
}
