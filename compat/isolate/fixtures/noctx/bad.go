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
