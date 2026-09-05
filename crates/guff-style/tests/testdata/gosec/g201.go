// G201 — SQL string formatting.
//
// gosec reaches this rule *indirectly*, and that indirection is the whole
// check. `sqlStrFormat.checkQuery` does not look at the query expression the
// call was given; it requires that expression to be a plain identifier, finds
// the `:=` that declared it **in the file containing the call**, and asks
// whether any right-hand side of that declaration is a risky `fmt` call:
//
//	ident, ok := query.(*ast.Ident)
//	if !ok { return nil, nil }
//	v, ok := ctx.Info.ObjectOf(ident).(*types.Var)
//	…
//	assign, ok := n.(*ast.AssignStmt)
//	if !ok || assign.Tok != token.DEFINE { return true }
//
// So a query built inline is not a finding, and neither is one that came from
// a `var` block or from package scope.
//
// `sqlStrFormat.Match` also has a branch `sqlStrConcat.Match` does not: the SQL
// call can be the **receiver** of the assigned call,
//
//	if sel, ok := call.Fun.(*ast.SelectorExpr); ok {
//		if sqlCall, ok := sel.X.(*ast.CallExpr); ok && s.ContainsCallExpr(sqlCall, ctx) != nil {
//			return s.checkQuery(sqlCall, ctx)
//		}
//	}
//
// which is how `err := db.QueryRow(q).Scan(&n)` is reached. telegraf's
// `plugins/outputs/sql/sql_test.go` has four of exactly that shape; guff had
// G201 in its DEFERRED list and reported none of them, so the four
// `//nolint:gosec` comments above them looked like unused directives and
// nolintlint reported those instead.
//
// The report is anchored at the `fmt` call, not at the database call.
package gosec

import (
	"database/sql"
	"fmt"
	"os"

	"github.com/lib/pq"
)

// --- reported ---

// The plain shape: a parameter reaches `%s` in a statement that starts with a
// SQL keyword.
func g201Param(db *sql.DB, table string) error {
	q := fmt.Sprintf("SELECT * FROM %s", table)
	_, err := db.Query(q)
	return err
}

// telegraf's shape. The SQL call is the receiver of `.Scan`, and only
// `sqlStrFormat` follows it there. Note the intermediate `x` and `y`: they are
// `fmt.Sprintf` results, so `TryResolve` fails on them and the outer call's
// arguments are not all safe.
func g201ThroughScanReceiver(db *sql.DB, a, b string) error {
	x := fmt.Sprintf("SELECT column_name FROM COLS WHERE t = '%s'", a)
	y := fmt.Sprintf("SELECT column_name FROM COLS WHERE t = '%s'", b)
	q := fmt.Sprintf("SELECT COUNT(*) FROM ((%s MINUS %s)) countdiff", x, y)
	var n int32
	err := db.QueryRow(q).Scan(&n)
	return err
}

// The `ExprStmt` arm of `Match`, on a `*sql.Tx`. The call has to stay a bare
// statement to reach that arm, so it also draws a G104 — that row is in the
// golden on purpose.
func g201ExprStmt(tx *sql.Tx, table string) {
	q := fmt.Sprintf("DELETE FROM %s WHERE k = 1", table)
	tx.Exec(q)
}

// `ConcatString` is not "is a string literal": it walks `+` and follows an
// identifier to the string values of *its* declaration, so the SQL keyword can
// live in a variable that never appears at the call.
func g201FormatBuiltFromIdent(db *sql.DB, table string) error {
	base := "SELECT * FROM "
	q := fmt.Sprintf(base+"%s", table)
	_, err := db.Query(q)
	return err
}

// `fmtCalls` holds four names, not one. `Sprintln` and `Sprint` do not
// interpret verbs, but the rule only ever matches the *text*, so a stray `%s`
// in their first argument is a finding all the same. (`go vet` complains about
// both lines; that is upstream's behaviour to reproduce, not a mistake.)
func g201Sprintln(db *sql.DB, table string) error {
	q := fmt.Sprintln("SELECT * FROM %s", table)
	_, err := db.Query(q)
	return err
}

func g201Sprint(db *sql.DB, table string) error {
	q := fmt.Sprint("SELECT * FROM %s", table)
	_, err := db.Query(q)
	return err
}

// The "all arguments are safe" exemption is all-or-nothing: one quoted
// identifier does not cover the unquoted `table` beside it.
func g201OneArgQuoted(db *sql.DB, table, col string) error {
	q := fmt.Sprintf("SELECT %s FROM %s", pq.QuoteIdentifier(col), table)
	_, err := db.Query(q)
	return err
}

// `Prepare` is in `sqlCallIdents` too, at argument 0.
func g201Prepare(db *sql.DB, table string) error {
	q := fmt.Sprintf("SELECT COUNT(*) FROM %s WHERE x = 1", table)
	_, err := db.Prepare(q)
	return err
}

// One assignment, two database calls, both risky — and *one* finding. Both
// arms of `Match` end in `return s.checkQuery(...)`, so the loop over `Rhs`
// stops at the first SQL call it recognises.
func g201TwoCallsOneFinding(db *sql.DB, a, b string) (*sql.Row, *sql.Row) {
	qa := fmt.Sprintf("SELECT * FROM %s", a)
	qb := fmt.Sprintf("SELECT * FROM %s", b)
	ra, rb := db.QueryRow(qa), db.QueryRow(qb)
	return ra, rb
}

// --- silent ---

// `Match` registers for `*ast.AssignStmt` and `*ast.ExprStmt` only. A `return`
// of the same receiver shape is neither, so nothing runs.
func g201ReturnStmt(db *sql.DB, a string) error {
	q := fmt.Sprintf("SELECT COUNT(*) FROM ((%s)) countdiff", a)
	var n int32
	return db.QueryRow(q).Scan(&n)
}

// Built inline: `checkQuery` wants an identifier.
func g201Inline(db *sql.DB, table string) error {
	_, err := db.Query(fmt.Sprintf("SELECT * FROM %s", table))
	return err
}

// Every argument after the format string resolves, so the construction is safe.
func g201ConstArgs(db *sql.DB) error {
	const table = "users"
	q := fmt.Sprintf("SELECT * FROM %s", table)
	_, err := db.Query(q)
	return err
}

// `sqlFormatRegexp` is `%[^bdoxXfFp]`: a number cannot carry a quote or a
// keyword, so `%d` is not a finding.
func g201NumericVerb(db *sql.DB, id int) error {
	q := fmt.Sprintf("SELECT * FROM t WHERE id = %d", id)
	_, err := db.Query(q)
	return err
}

// `sqlRegexp` wants a keyword followed by whitespace.
func g201NotSQL(db *sql.DB, s string) error {
	q := fmt.Sprintf("hello %s", s)
	_, err := db.Query(q)
	return err
}

// A `var` declaration is not the `:=` the rule searches for.
func g201VarDecl(db *sql.DB, table string) error {
	var q = fmt.Sprintf("SELECT * FROM %s", table)
	_, err := db.Query(q)
	return err
}

// `Sprint` without a verb: `sqlRegexp` matches but `sqlFormatRegexp` does not,
// and `MatchPatterns` needs both.
func g201SprintNoVerb(db *sql.DB, table string) error {
	q := fmt.Sprint("SELECT * FROM ", table)
	_, err := db.Query(q)
	return err
}

// `noIssueQuoted` — every argument is `pq.QuoteIdentifier`, so all safe.
func g201AllQuoted(db *sql.DB, col string) error {
	q := fmt.Sprintf("SELECT %s FROM users", pq.QuoteIdentifier(col))
	_, err := db.Query(q)
	return err
}

var g201PkgLevelQuery = fmt.Sprintf("SELECT * FROM %s", os.Getenv("T"))

// A package-level `var` is not a `:=` either, and `checkQuery` searches only
// the file holding the call — `sqlStrConcat` widens the search to the whole
// package for package-level variables, `sqlStrFormat` does not.
func g201PkgLevel(db *sql.DB) error {
	_, err := db.Query(g201PkgLevelQuery)
	return err
}

// No arguments at all, so no verb: `sqlFormatRegexp` fails.
func g201NoArgs(db *sql.DB) error {
	q := fmt.Sprintf("SELECT * FROM t")
	_, err := db.Query(q)
	return err
}
