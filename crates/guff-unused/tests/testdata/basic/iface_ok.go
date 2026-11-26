package iface

type Buffer interface {
	get() []byte
	set([]byte)
}

type syncBuf struct{ buf []byte }

func New() Buffer { return &syncBuf{} }
func (b *syncBuf) get() []byte { return b.buf }
func (b *syncBuf) set(p []byte) { b.buf = p }
func (b *syncBuf) trulyUnused() {}
