// Stub of `github.com/lib/pq`, for G201's `noIssueQuoted` table
// (`rule.noIssueQuoted.Add("github.com/lib/pq", "QuoteIdentifier")`). gosec
// matches on import path plus function name, so the body is irrelevant.
package pq

func QuoteIdentifier(name string) string { return `"` + name + `"` }
