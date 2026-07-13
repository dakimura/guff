package signal

type Signal interface{}

func Notify(c chan<- Signal, sig ...Signal) {}
