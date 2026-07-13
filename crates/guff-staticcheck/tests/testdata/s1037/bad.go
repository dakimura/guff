package main

import "time"

func f() {
	select {
	case t := <-time.After(time.Second):
		_ = t
	}
}
