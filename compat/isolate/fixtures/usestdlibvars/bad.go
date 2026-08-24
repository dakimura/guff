package p

import (
	"crypto"
	"database/sql"
	"net/http"
	"time"
)

// usestdlibvars does not flag a string that merely *looks* like a method: it
// walks specific call sites, struct fields and comparisons. A bare
// `return "GET"` is not one of them, which is why a fixture built from that
// reported nothing at all.

func Method() (*http.Request, error) {
	return http.NewRequest("GET", "http://example.com", nil)
}

func StatusCode(w http.ResponseWriter) {
	w.WriteHeader(200)
}

func Weekday(t time.Time) bool {
	return t.Weekday().String() == "Sunday"
}

func Month(t time.Time) bool {
	return t.Month().String() == "January"
}

func CryptoHash() crypto.Hash {
	var h crypto.Hash
	if h.String() == "MD5" {
		return h
	}
	return h
}

func IsolationLevel(l sql.IsolationLevel) bool {
	return l.String() == "Serializable"
}
