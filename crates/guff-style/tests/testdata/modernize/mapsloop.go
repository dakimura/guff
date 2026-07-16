//go:build go1.23

package modernize

func copyMap(dst, src map[int]string) {
	for k, v := range src {
		dst[k] = v
	}
}
