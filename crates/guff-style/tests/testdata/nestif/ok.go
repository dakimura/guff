package nestif

func Shallow(a, b bool) {
	if a {
		if b {
			return
		}
	}
}
