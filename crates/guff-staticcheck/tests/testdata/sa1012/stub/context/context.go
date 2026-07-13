package context

type Context interface{}

func TODO() Context {
	var c Context
	return c
}

func Background() Context {
	var c Context
	return c
}
