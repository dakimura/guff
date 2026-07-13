package signal

import "os"

func Notify(c chan os.Signal, sig ...os.Signal) {}
