package p

import (
	"database/sql"
	"net/http"
)

// noctx covers two families with two different messages: the net/http helpers
// that build a request without a context, and the database/sql methods that
// have a `…Context` twin.

func HTTPGet() error {
	_, err := http.Get("http://example.com")
	return err
}

func HTTPNewRequest() error {
	_, err := http.NewRequest("GET", "http://example.com", nil)
	return err
}

func SQLQuery(db *sql.DB) error {
	_, err := db.Query("select 1")
	return err
}

func SQLExec(db *sql.DB) error {
	_, err := db.Exec("select 1")
	return err
}

// A function literal in a package-level `var` initializer belongs to the
// synthesized package `init`, which has no *ast.FuncDecl — so `buildssa`
// never puts it in SrcFuncs and no SSA-based analyzer can reach it. Nothing
// below is reported, however wrong it looks. (Ginkgo suites are written
// exactly this way: `var _ = Describe("…", func() { … })`.)
func suite(name string, body func()) bool { return true }

var _ = suite("in a var initializer", func() {
	_, _ = http.Get("http://example.com/varinit")
	_, _ = http.NewRequest("GET", "http://example.com/varinit", nil)
	var db *sql.DB
	_, _ = db.Query("select 1")
})
