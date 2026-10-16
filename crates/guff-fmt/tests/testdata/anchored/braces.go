// Repeated lines: blank lines and closing braces. A minimal-edit diff is free
// to pair any `}` with any other; the anchored one only pairs unique lines.
package anchored

func G(x int) int {
	if x > 0 {
		return 1
	}

	if x < 0 {
		return -1

	}


	return 0
}

func H() {
}
