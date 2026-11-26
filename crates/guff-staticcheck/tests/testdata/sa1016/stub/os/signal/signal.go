package signal

import "os"

func Notify(c chan os.Signal, sig ...os.Signal) {}
func Ignore(sig ...os.Signal) {}
func Reset(sig ...os.Signal) {}
