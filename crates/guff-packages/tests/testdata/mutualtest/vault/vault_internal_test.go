package vault

import "example.com/mutual/session"

// Reads a test-only type of session whose only method is promoted from tool.
// If the seed strands tool in session's own wave, Use is not promoted and this
// is where it shows: "has no field or method Use".
var _ = session.Helper{}.Use()
