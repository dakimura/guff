// Package caller leaks rows twice and is skipped whole, because
// `getTargetTypes` asks the SSA program for `database/sql` and `buildssa` only
// creates SSA packages for `pass.Pkg.Imports()` — the **direct** imports:
//
//	pkg := pssa.Pkg.Prog.ImportedPackage(sqlPkg)
//	if pkg == nil {
//		// the SQL package being checked isn't imported
//		continue
//	}
//
// With no target types the analyzer returns before it reads an instruction.
// vitess's `test/client/client.go` is this shape and guff reported both of its
// unclosed `rows`.
//
// This file exists only for the golden case: the unit-test harness type-checks
// one package at a time and cannot express a second one.
package caller

import "example.com/sqlclosecheck-imports/driver"

func Leaks() error {
	db := driver.Open()
	rows, err := db.Query("select 1")
	if err != nil {
		return err
	}
	for rows.Next() {
	}
	return nil
}

func LeaksBlank() error {
	db := driver.Open()
	_, err := db.Query("select 1")
	return err
}
