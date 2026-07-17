package funcorder

type S struct{}

func NewBS() S { return S{} }

func NewAS() S { return S{} }

func (s S) GoodMorning() string { return "" }

func (s S) GoodAfternoon() string { return "" }

func (s S) hello() string { return "" }

func (s S) bye() string { return "" }
