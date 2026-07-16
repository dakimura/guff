//go:build go1.24

package modernize

import (
	"sync"
	"testing"
)

func BenchmarkA(b *testing.B) {
	println("slow")
	b.ResetTimer()

	for range b.N { // want
	}
}

func BenchmarkB(b *testing.B) {
	// setup
	{
		b.StopTimer()
		println("slow")
		b.StartTimer()
	}

	for i := range b.N { // nope: keyed range over b.N
		print(i)
	}

	b.StopTimer()
	println("slow")
}

func BenchmarkC(b *testing.B) {
	// setup
	{
		b.StopTimer()
		println("slow")
		b.StartTimer()
	}

	for i := 0; i < b.N; i++ { // want: unused i
		println("no uses of i")
	}

	b.StopTimer()
	println("slow")
}

func BenchmarkD(b *testing.B) {
	for i := 0; i < b.N; i++ { // want: i used
		println(i)
	}
}

func BenchmarkE(b *testing.B) {
	b.Run("sub", func(b *testing.B) {
		b.StopTimer() // not deleted
		println("slow")
		b.StartTimer() // not deleted

		// ...
	})
	b.ResetTimer()

	for i := 0; i < b.N; i++ { // want
		println("no uses of i")
	}

	b.StopTimer()
	println("slow")
}

func BenchmarkF(b *testing.B) {
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < b.N; i++ { // nope: b.N from FuncLit
		}
	}()
	wg.Wait()
}

func BenchmarkG(b *testing.B) {
	var wg sync.WaitGroup
	poster := func() {
		for i := 0; i < b.N; i++ { // nope: b.N from FuncLit
		}
		wg.Done()
	}
	wg.Add(2)
	for i := 0; i < 2; i++ {
		go poster()
	}
	wg.Wait()
}

func BenchmarkH(b *testing.B) {
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for range b.N { // nope: b.N from FuncLit
		}
	}()
	wg.Wait()
}

func BenchmarkI(b *testing.B) {
	for i := 0; i < b.N; i++ { // nope: multiple b.N
	}
	for i := 0; i < b.N; i++ { // nope: multiple b.N
	}
}

func BenchmarkJ(b *testing.B) {
	var wg sync.WaitGroup
	ch := make(chan int, 10)
	wg.Add(1)
	go func() {
		for i := 0; i < b.N; i++ {
			_ = <-ch
		}
		wg.Done()
	}()
	b.ResetTimer()
	for i := 0; i < b.N; i++ { // nope: multiple b.N
		ch <- i
	}
	b.StopTimer()
	wg.Wait()
}
