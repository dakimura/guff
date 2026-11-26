package main

import "testing"

func bad(t *testing.T) {
	go func() {
		t.Fatal()
	}()
}
