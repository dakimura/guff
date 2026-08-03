package main

func f(x int) {
	if x > 0 {
		return
	}
}

// Empty if with non-empty else must not report (upstream SA9003 / k8s valuefuzz).
func g(x int) {
	if x > 0 {
		// intentional empty
	} else {
		_ = x
	}
}
