package http

type ResponseWriter interface{}

type Handler interface{}

const StatusAccepted = 202
const StatusOK = 200

func Error(w ResponseWriter, error string, code int) {}
func Redirect(w ResponseWriter, r interface{}, url string, code int) {}
func StatusText(code int) string { return "" }
func RedirectHandler(url string, code int) Handler { return nil }
