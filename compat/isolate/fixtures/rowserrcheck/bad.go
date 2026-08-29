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

// A function literal in a package-level `var` initializer belongs to the
// synthesized package `init`, which has no *ast.FuncDecl — so `buildssa`
// never puts it in SrcFuncs and no SSA-based analyzer can reach it. Nothing
// below is reported, however wrong it looks. (Ginkgo suites are written
// exactly this way: `var _ = Describe("…", func() { … })`.)
func suite(name string, body func()) bool { return true }

var _ = suite("in a var initializer", func() {
	var db *sql.DB
	rows, err := db.Query("select 1")
	if err != nil {
		return
	}
	defer rows.Close()
	for rows.Next() {
	}
})
