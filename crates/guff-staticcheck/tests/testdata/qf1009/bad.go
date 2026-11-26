package pkg

import "time"

func foo() time.Time { return time.Time{} }
func bar() time.Time { return time.Time{} }

func fn() {
	var t1, t2 time.Time
	if t1 == t2 {
	}
	if foo() == bar() {
	}
}
