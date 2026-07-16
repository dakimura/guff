package exclude

import "io"

func useCopy(r io.Reader, w io.Writer) {
	io.Copy(w, r)
}

func useWriteString(w io.Writer) {
	io.WriteString(w, "x")
}
