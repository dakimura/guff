package main

type T struct{ X int }

func (t *T) Set() { t.X = 1 }

type Writer struct{ PredefinedACL string }
type writerWrapper struct{ *Writer }

// Value receiver over pointer embed: promoted field write is observable.
func (w writerWrapper) SetACL(acl string) { w.PredefinedACL = acl }
