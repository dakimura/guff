package varinit

// Package-level var initializers must not count as occurrences (upstream
// jgautheron/goconst has no ValueSpec path). With only two AssignStmt uses,
// this stays under the default min-occurrences=3 threshold.
var (
	scheme = "http"
)

func use() {
	_ = "http"
	_ = "http"
}
