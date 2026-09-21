// G202 — SQL string concatenation.
//
// `sqlStrConcat.checkQuery` has two branches and guff only had the first.
//
//	// Direct binary concatenation (e.g., "SELECT ..." + tainted)
//	if be, ok := query.(*ast.BinaryExpr); ok { … ; return nil, nil }
//
//	// Must be an identifier to continue (e.g., var query = ...; query += ...)
//	ident, ok := query.(*ast.Ident)
//
// The second branch is where beats' osquery browserhistory extension lives:
// three queries assembled as `SELECT …WHERE 1=1` + where + `ORDER BY …` and
// handed to `QueryContext`. The call's argument is an identifier, so the
// direct branch never looked at them.
//
// Upstream's order inside that branch is not the obvious one:
//
//  1. a risky concatenation *in the declaration* reports straight away, with
//     no SQL-pattern test — the call is already a SQL sink;
//  2. only then does `hasSQLPattern` gate the search for a later mutation;
//  3. that search accepts `q += tainted` and `q = q + tainted`, and nothing
//     else.
package gosec

import (
	"context"
	"database/sql"
)

// --- reported: the declaration concatenates ---

// One `+`.
func g202Single(db *sql.DB, where string) error {
	q := "SELECT a FROM t WHERE " + where
	_, err := db.Query(q)
	return err
}

// Two `+` with the tainted operand in the middle: `GetBinaryExprOperands`
// flattens the whole chain, so its position in the tree does not matter.
func g202Middle(ctx context.Context, db *sql.DB, where string) error {
	q := "SELECT a FROM t WHERE 1=1" + where + " ORDER BY a"
	_, err := db.QueryContext(ctx, q)
	return err
}

// Raw strings, which is how beats writes them.
func g202RawStrings(ctx context.Context, db *sql.DB, where string) error {
	q := `SELECT a FROM t WHERE 1=1` + where + `
	ORDER BY a`
	_, err := db.QueryContext(ctx, q)
	return err
}

// Extra arguments on the call change nothing.
func g202WithArgs(ctx context.Context, db *sql.DB, where string, args ...any) error {
	q := "SELECT a FROM t WHERE 1=1" + where + " ORDER BY a"
	_, err := db.QueryContext(ctx, q, args...)
	return err
}

// `Exec` is a sink too.
func g202Exec(db *sql.DB, where string) error {
	q := "SELECT a FROM t WHERE " + where
	_, err := db.Exec(q)
	return err
}

// --- reported: the declaration is clean and a later statement is not ---

// `q += tainted`, anchored at the assignment.
func g202Appended(ctx context.Context, db *sql.DB, where string) error {
	q := "SELECT a FROM t"
	q += where
	_, err := db.QueryContext(ctx, q)
	return err
}

// `q = q + tainted` reaches the same arm.
func g202SelfConcat(ctx context.Context, db *sql.DB, where string) error {
	q := "SELECT a FROM t"
	q = q + where
	_, err := db.QueryContext(ctx, q)
	return err
}

// --- silent ---

// Every operand resolves to a constant.
func g202ConstantsOnly(ctx context.Context, db *sql.DB) error {
	const w = " WHERE a = 1"
	q := "SELECT a FROM t" + w
	_, err := db.QueryContext(ctx, q)
	return err
}

// No SQL pattern in the declaration, so the later mutation is not looked at.
func g202NoSQLPattern(ctx context.Context, db *sql.DB, where string) error {
	q := "hello "
	q += where
	_, err := db.QueryContext(ctx, q)
	return err
}

// A mutation that stays constant.
func g202ConstantMutation(ctx context.Context, db *sql.DB) error {
	q := "SELECT a FROM t"
	q += " WHERE a = 1"
	_, err := db.QueryContext(ctx, q)
	return err
}
