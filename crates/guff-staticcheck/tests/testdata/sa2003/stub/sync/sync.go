package sync

type Mutex struct{}

func (m *Mutex) Lock()   {}
func (m *Mutex) Unlock() {}

type RWMutex struct{}

func (m *RWMutex) Lock()    {}
func (m *RWMutex) Unlock()  {}
func (m *RWMutex) RLock()   {}
func (m *RWMutex) RUnlock() {}
