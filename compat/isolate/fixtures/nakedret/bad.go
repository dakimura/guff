package p

func Bad() (n int) {
	n = 1
	return
}

// nakedret names the function and the line count, so two functions over the
// limit are two different sentences.
func AlsoBad() (s string, err error) {
	s = "x"
	err = nil

	return
}
