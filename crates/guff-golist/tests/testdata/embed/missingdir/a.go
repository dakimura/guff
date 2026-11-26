package missingdir

import "embed"

//go:embed app/dist
var asset embed.FS

// Asset is alertmanager's `ui/web.go` shape: the directory is produced by a
// build step (`make ui`) and is absent from a plain checkout.
func Asset() embed.FS { return asset }
