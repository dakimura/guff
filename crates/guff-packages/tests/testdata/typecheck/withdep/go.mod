module example.com/withdep

go 1.22

require example.com/simple v0.0.0

replace example.com/simple => ../guff-exportdata/tests/testdata/export/simple
