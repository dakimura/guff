package quoted

import "embed"

//go:embed "no such.txt"
var asset embed.FS

// Asset pins the quoted argument form: the message carries the unquoted text.
func Asset() embed.FS { return asset }
