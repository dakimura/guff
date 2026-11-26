package assert

func ok() {
	var i interface{}
	s, ok := i.(string)
	if !ok {
		return
	}
	_ = s
}
