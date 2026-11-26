package usetesting

import (
	"os"
	"testing"
)

func TestBad(t *testing.T) {
	_, _ = os.MkdirTemp("", "prefix")
	_, _ = os.CreateTemp("", "prefix")
}
