package gochecknoinitsok

type T struct{}

// init as a method (has a receiver) is fine.
func (T) init() {}

// A regular function named differently is fine.
func initialize() {}

func regular() {}
