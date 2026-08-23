package p

type T struct{ x int }

func New(v int) T { return T{x: v} }

func (t T) Half() int { return t.x / 2 }
