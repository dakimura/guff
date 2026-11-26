package pkg

import "time"

type T1 struct {
	aMS     int
	B       time.Duration
	BMillis time.Duration
}

func fn1(a, b, cMS time.Duration) {
	var x time.Duration
	var xMS time.Duration
	var y, yMS time.Duration
	var zMS = time.Second
	aMS := time.Second
	unrelated, aMS2 := 0, 0
	aMS3, bMS := 0, time.Second

	_, _, _, _, _, _, _, _, _, _ = x, xMS, y, yMS, zMS, aMS, unrelated, aMS2, aMS3, bMS
}
