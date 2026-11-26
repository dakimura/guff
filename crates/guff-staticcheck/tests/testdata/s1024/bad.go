package main

import "time"

func f(t time.Time) time.Duration { return t.Sub(time.Now()) }
