package loggercheck

func MyLog(msg string, keysAndValues ...any) {}

func customBad() {
	MyLog("msg", "key1")
	MyLog("msg", "key1", "value1", "key2")
}

func customOk() {
	MyLog("msg", "key1", "value1")
}
