package bad

import "example.com/govet/composites/other"

func bad() {
	_ = other.Config{nil}
}
