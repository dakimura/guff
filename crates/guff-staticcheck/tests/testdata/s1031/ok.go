package main

func f(s []int) {
	for range s {
	}
}

// `(IfStmt nil cond [range] nil)` — the trailing nil is the else branch. With
// one, dropping the check is a different program: dapr's `default_bulksub.go`
// runs an "all messages failed" path instead.
func withElse(s []int) int {
	total := 0
	if s != nil {
		for _, x := range s {
			total += x
		}
	} else {
		total = -1
	}
	return total
}
