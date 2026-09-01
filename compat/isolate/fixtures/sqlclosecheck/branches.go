package p

import "database/sql"

// Upstream decides on the SSA value: two assignments in the arms of an `if`
// meet in one φ, and one close settles every edge into it. This port tracks a
// name in a statement list instead, so the second assignment orphaned the
// first and reported it — syncthing's `PrefixKV`, twice over.
//
// A *sequential* reassignment is not that: the first really does lose its
// rows, and upstream reports it even though a close follows the second.

// Silent: the arms meet, and the close after them settles both.
func BranchesClosedAfter(db *sql.DB, prefix string) error {
	var rows *sql.Rows
	var err error
	if prefix == "" {
		rows, err = db.Query("select 1")
	} else {
		rows, err = db.Query("select 2", prefix)
	}
	if err != nil {
		return err
	}
	defer rows.Close()
	return nil
}

// Reported twice: nothing settles either arm.
func BranchesNeverClosed(db *sql.DB, prefix string) error {
	var rows *sql.Rows
	var err error
	if prefix == "" {
		rows, err = db.Query("select 1")
	} else {
		rows, err = db.Query("select 2", prefix)
	}
	_ = rows
	return err
}

// Reported once, at the first: two assignments in the same list run in
// sequence, so the close only reaches the second.
func SequentialReassign(db *sql.DB) error {
	rows, err := db.Query("select 1")
	if err != nil {
		return err
	}
	rows, err = db.Query("select 2")
	if err != nil {
		return err
	}
	defer rows.Close()
	return nil
}

// Silent: closed inside a literal that is handed back rather than deferred
// here. Only `defer func(){ … }()` at the site used to count.
func ClosedInReturnedClosure(db *sql.DB) (func(), error) {
	rows, err := db.Query("select 1")
	if err != nil {
		return nil, err
	}
	return func() {
		defer rows.Close()
		for rows.Next() {
		}
	}, nil
}

// Reported: captured by a literal that never closes it. A capture on its own
// settles nothing here, which is the difference from bodyclose.
func CapturedNotClosed(db *sql.DB) (func(), error) {
	rows, err := db.Query("select 1")
	if err != nil {
		return nil, err
	}
	return func() {
		for rows.Next() {
		}
	}, nil
}
