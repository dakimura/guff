package pkg

import "time"

func fn() {
	for range time.Tick(0) {
		println("")
		if true {
			break
		}
	}
}
