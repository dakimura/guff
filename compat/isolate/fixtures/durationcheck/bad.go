package p

import "time"

func Bad(d time.Duration) time.Duration {
	return d * time.Second // duration * duration
}

// durationcheck reports the multiplication it found, and the message quotes the
// expression — so each shape is a different sentence.
func Reversed(d time.Duration) time.Duration {
	return time.Second * d
}

func BothVars(a, b time.Duration) time.Duration {
	return a * b
}
