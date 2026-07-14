package main

import (
	"github.com/dakimura/guff/benchmarks/local/pkg00"
	"github.com/dakimura/guff/benchmarks/local/pkg01"
	"github.com/dakimura/guff/benchmarks/local/pkg02"
	"github.com/dakimura/guff/benchmarks/local/pkg03"
	"github.com/dakimura/guff/benchmarks/local/pkg04"
	"github.com/dakimura/guff/benchmarks/local/pkg05"
	"github.com/dakimura/guff/benchmarks/local/pkg06"
	"github.com/dakimura/guff/benchmarks/local/pkg07"
	"github.com/dakimura/guff/benchmarks/local/pkg08"
	"github.com/dakimura/guff/benchmarks/local/pkg09"
	"github.com/dakimura/guff/benchmarks/local/pkg10"
	"github.com/dakimura/guff/benchmarks/local/pkg11"
)

func main() {
	_ = pkg00.Work2(10)
	pkg00.Use0()
	_ = pkg01.Work2(10)
	pkg01.Use0()
	_ = pkg02.Work2(10)
	pkg02.Use0()
	_ = pkg03.Work2(10)
	pkg03.Use0()
	_ = pkg04.Work2(10)
	pkg04.Use0()
	_ = pkg05.Work2(10)
	pkg05.Use0()
	_ = pkg06.Work2(10)
	pkg06.Use0()
	_ = pkg07.Work2(10)
	pkg07.Use0()
	_ = pkg08.Work2(10)
	pkg08.Use0()
	_ = pkg09.Work2(10)
	pkg09.Use0()
	_ = pkg10.Work2(10)
	pkg10.Use0()
	_ = pkg11.Work2(10)
	pkg11.Use0()
}
