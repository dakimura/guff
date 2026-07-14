package forcetypeassert

func ok() {
	var a any
	if v, ok := a.(int); ok {
		_ = v
	}
}
