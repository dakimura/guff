package time

type Duration int64
const Second Duration = 1
func After(Duration) <-chan Time { ch := make(chan Time); return ch }
func Sleep(Duration) {}
type Time struct{}
