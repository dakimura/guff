package err113

func ok(e1, e2 error) bool {
	return e1 != nil && e2 != nil
}
