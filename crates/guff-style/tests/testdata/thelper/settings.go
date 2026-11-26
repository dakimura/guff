package thelpersettings

import "testing"

func helperWithoutHelper(t *testing.T) {
	// begin disabled via settings — should not report
}

func helperWrongName(o *testing.T) {
	o.Helper()
	// name still enabled — should report
}
