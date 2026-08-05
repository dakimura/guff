package p

import (
	"crypto/md5"
	"math/rand"
	"net/http"
	"os"
)

func BadHash() {
	_ = md5.New()
}

func BadRand() {
	_ = rand.Int() // weak random
}

func BadHTTP() {
	http.ListenAndServe(":8080", nil) // G114-ish bind
}

func BadPathTraversal() {
	if path := os.Getenv("DEBUG_FILE"); path != "" {
		_, _ = os.OpenFile(path, 0, 0) // G703
	}
}

func BadSliceBounds() {
	s := make([]byte, 0)
	_ = s[:3] // G602
}
