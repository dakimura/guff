package messages

// One import per message shape upstream's `BlockReason` can produce. gomodguard
// reports the import spec, so the package bodies are never read.

import (
	_ "example.com/bare"
	_ "example.com/onerec"
	_ "example.com/threerecs"
	_ "example.com/tworecs"
	_ "example.com/verbs"
)
