package sync

type WaitGroup struct{}

func (wg *WaitGroup) Add(delta int) {}
func (wg *WaitGroup) Done()         {}
func (wg *WaitGroup) Wait()         {}
