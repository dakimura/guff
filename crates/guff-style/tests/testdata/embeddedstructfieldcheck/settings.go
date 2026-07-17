package settings

import "sync"

type MutexEmbedded struct {
	sync.Mutex
}

type PointerMutexEmbedded struct {
	*sync.Mutex
}

type RWMutexEmbedded struct {
	sync.RWMutex
}

type MutexNotEmbedded struct {
	mu sync.Mutex
}

type NoSpaceWhenEmptyLineOff struct {
	EmbedMe
	version int
}

type EmbedMe struct {
	N int
}
