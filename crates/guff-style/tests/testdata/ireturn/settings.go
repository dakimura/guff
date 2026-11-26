package ireturn

// Used with reject: [empty] — should flag interface{}.
func ReturnsEmpty() interface{} {
	return 1
}

// Named interface is allowed under reject:[empty].
type Local interface {
	M()
}

func ReturnsLocal() Local {
	return nil
}
