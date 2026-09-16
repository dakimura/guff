package synctest

import "testing"

// Test takes the subtest literal in the same position t.Run does — argument
// index 1, one *testing.T parameter — and is still not t.Run.
func Test(t *testing.T, f func(*testing.T)) {}
