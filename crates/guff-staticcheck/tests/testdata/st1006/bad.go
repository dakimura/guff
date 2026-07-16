package pkg

type T1 int

func (x T1) Fn1()    {}
func (T1) Fn4()      {}
func (_ T1) Fn5()    {}
func (self T1) Fn6() {}
func (this T1) Fn7() {}
