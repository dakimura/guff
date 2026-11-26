// Package istest pins revive's `IsTest()` to a filename, not a package name.
//
// The rules that ask are split across both files on purpose: a rule that skips
// test files must still fire here, and a rule that only runs in them must not.
package istest

// Endpoint is in a non-test file, so unsecure-url-scheme reports it.
const Endpoint = "http://example.com/v1"
