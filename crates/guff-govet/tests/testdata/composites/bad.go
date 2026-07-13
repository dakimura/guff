package bad

import "other"

func bad() {
	_ = other.Config{nil}
}
