package testonly

import (
	"embed"
	"testing"
)

//go:embed testmissing
var asset embed.FS

func TestAsset(t *testing.T) { _ = asset }
