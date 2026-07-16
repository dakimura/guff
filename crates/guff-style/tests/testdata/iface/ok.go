package ifaceok

type Reader interface {
	Read([]byte) (int, error)
}

type Writer interface {
	Write([]byte) (int, error)
}

func use(r Reader, w Writer) {
	_, _ = r.Read(nil)
	_, _ = w.Write(nil)
}
