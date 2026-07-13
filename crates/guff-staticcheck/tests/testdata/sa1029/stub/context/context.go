package context

type Context interface{}

func WithValue(parent Context, key, val any) Context { return parent }
