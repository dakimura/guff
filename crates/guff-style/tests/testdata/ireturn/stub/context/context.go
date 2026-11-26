package context

type Context interface{}

func Background() Context {
	return nil
}
