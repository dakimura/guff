module example.com/exhaustiveignore

go 1.25

// The dependency module is never analysed by the run: guff has to read its
// declaration directives from source.
require example.com/enumdep v0.0.0

replace example.com/enumdep => ./enumdep
