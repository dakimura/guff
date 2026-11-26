package pkg

import "fmt"

type NotAWriter struct{}

func (NotAWriter) Write(b []byte) {}

type Writer interface {
	Write([]byte) (int, error)
}

func fn1() {
	var w Writer
	var w2 NotAWriter

	w.Write([]byte(fmt.Sprint("abc", "de")))
	w.Write([]byte(fmt.Sprintf("%T", w)))
	w.Write([]byte(fmt.Sprintln("abc", "de")))

	w2.Write([]byte(fmt.Sprint("abc", "de")))
}

func fn2() {
	var buf Buffer
	buf.WriteString(fmt.Sprint("abc", "de"))
	buf.WriteString(fmt.Sprintf("%T", 0))
	buf.WriteString(fmt.Sprintln("abc", "de"))
}

type Buffer struct{}

func (*Buffer) Write(b []byte) (int, error)       { return 0, nil }
func (*Buffer) WriteString(s string) (int, error) { return 0, nil }
