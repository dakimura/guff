package nosprintfhostport

import "fmt"

func bad(host string, port int) string {
	return fmt.Sprintf("http://%s:%d", host, port)
}

func badAuth(user, host, port string) string {
	return fmt.Sprintf("https://%s@%s:%s", user, host, port)
}
