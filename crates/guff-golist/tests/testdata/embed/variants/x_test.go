package variants_test

import (
	"embed"
	"testing"
)

//go:embed xtestmissing
var external embed.FS

func TestExternal(t *testing.T) { _ = external }
