package missingfile

import _ "embed"

//go:embed missing.txt
var text string

// Text pins the blank import: `import _ "embed"` is still an import of embed.
func Text() string { return text }
