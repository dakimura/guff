package p

import "fmt"

func Bad(host string, port int) string {
	return fmt.Sprintf("http://%s:%d", host, port)
}

// nosprintfhostport walks the format string, so each shape that builds a
// host:port by hand is its own site.
func WithScheme(host string, port int) string {
	return fmt.Sprintf("https://%s:%d/path", host, port)
}

func NoScheme(host string, port int) string {
	return fmt.Sprintf("%s:%d", host, port)
}

func IPv6Safe(host string, port int) string {
	// Bracketed IPv6 is the shape upstream accepts.
	return fmt.Sprintf("http://[%s]:%d", host, port)
}
