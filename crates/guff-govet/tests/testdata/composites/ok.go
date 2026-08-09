package ok

import "example.com/govet/composites/other"

func ok() {
	_ = other.Config{Err: nil}
}
