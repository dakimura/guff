package settings

import "b"

func reassignAll() {
	b.ErrB = nil
	b.NotErr = "flagged when patterns is .*"
}
