// gosec v2.29.0 `resolveCallExpr`: `b.String()` on a local strings.Builder or
// bytes.Buffer resolves to a constant when every write to `b` in the file
// resolves. Both TryResolve ports reach it — G202's (the SQL concatenation)
// and the declaration-walking one G204 uses — and each branch of
// `builderWritesAreConst` has a shape here.
package g202b

import (
	"bytes"
	"context"
	"database/sql"
	"os/exec"
	"strings"
)

// Every write is a constant: resolves, silent.
func constWrites(ctx context.Context, db *sql.DB) {
	var b strings.Builder
	b.WriteString(" WHERE a = ")
	b.WriteByte('1')
	b.WriteRune(' ')
	_ = b.Len()
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+b.String())
}

// A non-constant write: does not resolve.
func varWrite(ctx context.Context, db *sql.DB, where string) {
	var b strings.Builder
	b.WriteString(where)
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+b.String())
}

// `:=` from an empty composite literal (through `&`) is a known-empty start.
func emptyLit(ctx context.Context, db *sql.DB) {
	b := &bytes.Buffer{}
	b.WriteString(" WHERE a = 1")
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+b.String())
}

// `:=` from anything else is opaque.
func seeded(ctx context.Context, db *sql.DB) {
	b := bytes.NewBufferString(" WHERE a = 1")
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+b.String())
}

// A method outside the known list makes the contents unknown.
func unknownMethod(ctx context.Context, db *sql.DB) {
	var b bytes.Buffer
	b.WriteString(" WHERE a = 1")
	b.Truncate(3)
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+b.String())
}

func fill(b *strings.Builder) { b.WriteString(" x") }

// The builder's address escapes: an unaccounted reference.
func escapes(ctx context.Context, db *sql.DB) {
	var b strings.Builder
	fill(&b)
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+b.String())
}

// A package-level builder may be written anywhere: never resolves.
var pkgBuilder strings.Builder

func packageLevel(ctx context.Context, db *sql.DB) {
	pkgBuilder.WriteString(" WHERE a = 1")
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+pkgBuilder.String())
}

// G204 through the declaration-walking TryResolve.
func command() {
	var b strings.Builder
	b.WriteString("ls")
	_ = exec.Command(b.String()).Run()
}

func commandVar(name string) {
	var b strings.Builder
	b.WriteString(name)
	_ = exec.Command(b.String()).Run()
}

// A string-typed `String()` on something else is still an unresolved call.
type named struct{}

func (named) String() string { return "x" }

func otherString(ctx context.Context, db *sql.DB) {
	var n named
	_, _ = db.QueryContext(ctx, "SELECT a FROM t"+n.String())
}
