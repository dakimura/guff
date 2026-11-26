package main

import (
	"sync"
	"unsafe"
)

// The pointer-like half of the grid in bad.go: a map, a channel, a func value
// and an `unsafe.Pointer` are each already one word, so `Put` boxes them
// without allocating and upstream says nothing. Named types follow their
// underlying type, and an interface — including `error` — is pointer-like too.

type okBox struct{ n int }

type namedMap map[string]int

type namedChan chan int

func f(p *sync.Pool, s []int) { p.Put(&s) }

func putPointer(p *sync.Pool, v *okBox) { p.Put(v) }

func putSlicePointer(p *sync.Pool, v *[]int) { p.Put(v) }

func putMap(p *sync.Pool, v map[string]int) { p.Put(v) }

func putNamedMap(p *sync.Pool, v namedMap) { p.Put(v) }

func putChan(p *sync.Pool, v chan int) { p.Put(v) }

func putNamedChan(p *sync.Pool, v namedChan) { p.Put(v) }

func putFunc(p *sync.Pool, v func()) { p.Put(v) }

func putUnsafePointer(p *sync.Pool, v unsafe.Pointer) { p.Put(v) }

func putAny(p *sync.Pool, v any) { p.Put(v) }

func putError(p *sync.Pool, v error) { p.Put(v) }
