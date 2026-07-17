package pkg

type T1 int

func (x T1) Fn1()    {}
func (y T1) Fn2()    {}
func (x T1) Fn3()    {}
func (T1) Fn4()      {}
func (_ T1) Fn5()    {}
func (self T1) Fn6() {}

type T3 struct{}

func (bar T3) Fn2()  {}
func (meow T3) Fn3() {}

type T4 struct{}

func (bar T4) Fn2() {}
