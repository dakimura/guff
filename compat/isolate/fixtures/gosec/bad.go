package p

import (
	"crypto/md5"
	"math/rand"
	"net/http"
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
