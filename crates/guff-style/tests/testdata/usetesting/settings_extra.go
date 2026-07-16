package usetesting

import (
	"os"
	"testing"
)

func TestSettingsExtra(t *testing.T) {
	_ = os.Setenv("K", "V")
	_ = os.TempDir()
}
