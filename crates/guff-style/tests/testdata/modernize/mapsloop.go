//go:build go1.23

package modernize

func copyMap(dst, src map[int]string) {
	for k, v := range src {
		dst[k] = v
	}
}

// The destination is a *call result*, indexed. syncthing `lib/httpcache` writes
// `w.Header()[k] = v`, and guff answered nothing — not because the shape was
// rejected but because the fix text could not be built: rendering the syntax
// tree by hand had no case for a call with **no** arguments, and a failure
// there drops the diagnostic.
type headers map[string][]string

type recorder struct{ header headers }

func (r *recorder) writeHeader(w interface{ Header() headers }) {
	for k, v := range r.header {
		w.Header()[k] = v
	}
}
