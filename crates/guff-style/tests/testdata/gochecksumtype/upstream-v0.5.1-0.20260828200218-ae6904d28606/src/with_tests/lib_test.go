package with_tests

import "testing"

func TestXxx(t *testing.T) {
	switch T(nil).(type) { // want `exhaustiveness check failed for sum type "T" .*missing cases for B`
	case *A:
	}
}
