package variants

import (
	"embed"
	"testing"
)

//go:embed testmissing
var internal embed.FS

func TestInternal(t *testing.T) { _ = internal }
