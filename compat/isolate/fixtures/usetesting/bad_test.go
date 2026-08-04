package p

import (
	"os"
	"testing"
)

func TestBad(t *testing.T) {
	_, _ = os.MkdirTemp("", "x")
}
