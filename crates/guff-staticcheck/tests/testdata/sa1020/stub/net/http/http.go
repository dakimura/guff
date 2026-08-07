package http

type Handler interface{}

func ListenAndServe(addr string, handler any) error {
	var err error
	return err
}

func ListenAndServeTLS(addr, certFile, keyFile string, handler any) error {
	var err error
	return err
}
