package atomic

type Int32 struct{}
type Int64 struct{}
type Uint32 struct{}
type Uint64 struct{}
type Uintptr struct{}

func (x *Int32) Add(delta int32) (new int32)             { return 0 }
func (x *Int32) Load() int32                             { return 0 }
func (x *Int32) Store(val int32)                         {}
func (x *Int32) Swap(new int32) (old int32)              { return 0 }
func (x *Int32) CompareAndSwap(old, new int32) bool      { return false }
func (x *Int32) And(mask int32) (old int32)              { return 0 }
func (x *Int32) Or(mask int32) (old int32)               { return 0 }

func (x *Int64) Add(delta int64) (new int64)             { return 0 }
func (x *Int64) Load() int64                             { return 0 }
func (x *Int64) Store(val int64)                         {}
func (x *Int64) Swap(new int64) (old int64)              { return 0 }
func (x *Int64) CompareAndSwap(old, new int64) bool      { return false }

func (x *Uint32) Add(delta uint32) (new uint32)          { return 0 }
func (x *Uint32) Load() uint32                           { return 0 }
func (x *Uint32) Store(val uint32)                       {}
func (x *Uint32) Swap(new uint32) (old uint32)           { return 0 }
func (x *Uint32) CompareAndSwap(old, new uint32) bool    { return false }

func (x *Uint64) Add(delta uint64) (new uint64)          { return 0 }
func (x *Uint64) Load() uint64                           { return 0 }
func (x *Uint64) Store(val uint64)                       {}
func (x *Uint64) Swap(new uint64) (old uint64)           { return 0 }
func (x *Uint64) CompareAndSwap(old, new uint64) bool    { return false }

func (x *Uintptr) Add(delta uintptr) (new uintptr)       { return 0 }
func (x *Uintptr) Load() uintptr                         { return 0 }
func (x *Uintptr) Store(val uintptr)                     {}
func (x *Uintptr) Swap(new uintptr) (old uintptr)        { return 0 }
func (x *Uintptr) CompareAndSwap(old, new uintptr) bool  { return false }

func AddInt32(addr *int32, delta int32) (new int32)                         { return 0 }
func AddInt64(addr *int64, delta int64) (new int64)                         { return 0 }
func AddUint32(addr *uint32, delta uint32) (new uint32)                     { return 0 }
func AddUint64(addr *uint64, delta uint64) (new uint64)                     { return 0 }
func AddUintptr(addr *uintptr, delta uintptr) (new uintptr)                 { return 0 }

func CompareAndSwapInt32(addr *int32, old, new int32) (swapped bool)        { return false }
func CompareAndSwapInt64(addr *int64, old, new int64) (swapped bool)        { return false }
func CompareAndSwapUint32(addr *uint32, old, new uint32) (swapped bool)     { return false }
func CompareAndSwapUint64(addr *uint64, old, new uint64) (swapped bool)     { return false }
func CompareAndSwapUintptr(addr *uintptr, old, new uintptr) (swapped bool)  { return false }

func LoadInt32(addr *int32) (val int32)           { return 0 }
func LoadInt64(addr *int64) (val int64)           { return 0 }
func LoadUint32(addr *uint32) (val uint32)        { return 0 }
func LoadUint64(addr *uint64) (val uint64)        { return 0 }
func LoadUintptr(addr *uintptr) (val uintptr)     { return 0 }

func StoreInt32(addr *int32, val int32)           {}
func StoreInt64(addr *int64, val int64)           {}
func StoreUint32(addr *uint32, val uint32)        {}
func StoreUint64(addr *uint64, val uint64)        {}
func StoreUintptr(addr *uintptr, val uintptr)     {}

func SwapInt32(addr *int32, new int32) (old int32)             { return 0 }
func SwapInt64(addr *int64, new int64) (old int64)             { return 0 }
func SwapUint32(addr *uint32, new uint32) (old uint32)         { return 0 }
func SwapUint64(addr *uint64, new uint64) (old uint64)         { return 0 }
func SwapUintptr(addr *uintptr, new uintptr) (old uintptr)     { return 0 }

func AndInt32(addr *int32, mask int32) (old int32)             { return 0 }
func AndInt64(addr *int64, mask int64) (old int64)             { return 0 }
func AndUint32(addr *uint32, mask uint32) (old uint32)         { return 0 }
func AndUint64(addr *uint64, mask uint64) (old uint64)         { return 0 }
func AndUintptr(addr *uintptr, mask uintptr) (old uintptr)     { return 0 }

func OrInt32(addr *int32, mask int32) (old int32)              { return 0 }
func OrInt64(addr *int64, mask int64) (old int64)              { return 0 }
func OrUint32(addr *uint32, mask uint32) (old uint32)          { return 0 }
func OrUint64(addr *uint64, mask uint64) (old uint64)          { return 0 }
func OrUintptr(addr *uintptr, mask uintptr) (old uintptr)      { return 0 }
