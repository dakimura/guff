package sync

type Func func()

type WaitGroup struct{}

func (wg *WaitGroup) Add(delta int) {}
func (wg *WaitGroup) Done()         {}
func (wg *WaitGroup) Wait()         {}
func (wg *WaitGroup) Go(f Func)     {}
