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
	for rows.Next() {
	}
})

// A target stored into a **struct field** is settled. Upstream's `*ssa.Store`
// arm says so in as many words:
//
//	case *ssa.Store:
//		// A Row/Stmt is stored in a struct, which may be closed later
//		// by a different flow.
//		if _, ok := instr.Addr.(*ssa.FieldAddr); ok {
//			return actionReturned
//		}
//
// telegraf's `plugins/inputs/sql/sql.go:327` prepares its statements into
// `s.Queries[i].statement` and closes them from `Stop`. Only a `FieldAddr`
// counts: a slice element, a map entry, a pointer indirection and a plain
// second variable are all other instructions, and all still findings.

type stored struct {
	stmt *sql.Stmt
	rows *sql.Rows
}

type keeper struct {
	db      *sql.DB
	entries []stored
	one     stored
	arr     []*sql.Stmt
	byName  map[string]*sql.Stmt
}

// Silent: a field of a slice element — telegraf's shape.
func (k *keeper) IntoSliceElemField() {
	for i := range k.entries {
		stmt, err := k.db.Prepare("select 1")
		if err != nil {
			continue
		}
		k.entries[i].stmt = stmt
	}
}

// Silent: a plain field.
func (k *keeper) IntoField() {
	stmt, err := k.db.Prepare("select 1")
	if err != nil {
		return
	}
	k.one.stmt = stmt
}

// Silent: a field on a local struct value is a `FieldAddr` too.
func (k *keeper) IntoLocalStructField() {
	stmt, err := k.db.Prepare("select 1")
	if err != nil {
		return
	}
	var s stored
	s.stmt = stmt
	_ = s
}

// Silent: rows, not just statements.
func (k *keeper) RowsIntoField() {
	rows, err := k.db.Query("select 1")
	if err != nil {
		return
	}
	k.one.rows = rows
}

// Reported: a slice element is an `IndexAddr`, not a `FieldAddr`.
func (k *keeper) IntoSliceElem() {
	stmt, err := k.db.Prepare("select 1")
	if err != nil {
		return
	}
	k.arr[0] = stmt
}

// Reported: a map entry is a `MapUpdate`, not a `Store` at all.
func (k *keeper) IntoMapEntry() {
	stmt, err := k.db.Prepare("select 1")
	if err != nil {
		return
	}
	k.byName["x"] = stmt
}

// Reported: through a pointer the destination is the pointer's own value.
func (k *keeper) ThroughPointer(p **sql.Stmt) {
	stmt, err := k.db.Prepare("select 1")
	if err != nil {
		return
	}
	*p = stmt
}

// Reported once, not twice. `getTargetTypesValues` starts from an `*ssa.Call`
// and nothing else, so the plain copy never becomes a value of its own.
func (k *keeper) CopiedToAnotherLocal() {
	stmt, err := k.db.Prepare("select 1")
	if err != nil {
		return
	}
	var y *sql.Stmt
	y = stmt
	_ = y
}
