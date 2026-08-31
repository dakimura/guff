package main

import "sync"

// SA6002 is `!typeutil.IsPointerLike(typ) || isSlice`, so the predicate is the
// check. These are the shapes that are *not* pointer-like — boxing them into
// the `any` that `Put` takes has to allocate — plus the slice, which is
// pointer-like and reported anyway because its header is three words.
//
// Sixteen shapes measured against golangci-lint 2.12.2; the other ten are in
// ok.go. Reading the predicate as "pointer or interface" reported six of those
// ten, which is six of fiber's nine staticcheck findings.

type box struct{ n int }

type namedSlice []int

func f(p *sync.Pool, s []int) { p.Put(s) }

func putNamedSlice(p *sync.Pool, v namedSlice) { p.Put(v) }

func putStruct(p *sync.Pool, v box) { p.Put(v) }

func putArray(p *sync.Pool, v [4]int) { p.Put(v) }

func putInt(p *sync.Pool, v int) { p.Put(v) }

func putString(p *sync.Pool, v string) { p.Put(v) }
