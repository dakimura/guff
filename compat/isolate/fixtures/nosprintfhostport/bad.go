package p

import "fmt"

func Bad(host string, port int) string {
	return fmt.Sprintf("http://%s:%d", host, port)
}
