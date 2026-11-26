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

func BadConversion(i int) int32 {
	return int32(i) // G115
}

// The guarded twin: G115's range analysis has to keep this one silent, which is
// the half of the rule an isolate diff can actually catch regressing.
func OkConversion(i int) int32 {
	if i > 2147483647 || i < -2147483648 {
		return 0
	}
	return int32(i)
}
