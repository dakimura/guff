package sync

type WaitGroup struct{}

func (wg *WaitGroup) Add(delta int) {}
func (wg *WaitGroup) Done()         {}
func (wg *WaitGroup) Wait()         {}

type Map struct{}

func (m *Map) Load(key interface{}) (value interface{}, ok bool) { return nil, false }
func (m *Map) Delete(key interface{})                            {}
func (m *Map) LoadAndDelete(key interface{}) (value interface{}, loaded bool) {
	return nil, false
}

func OnceFunc(f func()) func() { return f }

type Mutex struct{}

func (m *Mutex) Lock()    {}
func (m *Mutex) Unlock()  {}

type RWMutex struct{}

func (m *RWMutex) Lock()    {}
func (m *RWMutex) Unlock()  {}
func (m *RWMutex) RLock()   {}
func (m *RWMutex) RUnlock() {}
