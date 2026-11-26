package pkg

type T1 int

func (t T1) Fn1() {}
func (t T1) Fn2() {}
func (T1) Fn3()   {}
func (_ T1) Fn4() {}

type T2 struct{}

func (t *T2) A() {}
func (t T2) B()  {}
