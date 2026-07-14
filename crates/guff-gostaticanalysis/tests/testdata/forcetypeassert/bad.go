package forcetypeassert

func bad() {
	var a any
	_ = a.(int)
}
