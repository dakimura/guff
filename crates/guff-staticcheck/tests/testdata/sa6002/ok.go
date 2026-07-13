package main
import "sync"
func f(p *sync.Pool, s []int) { p.Put(&s) }
