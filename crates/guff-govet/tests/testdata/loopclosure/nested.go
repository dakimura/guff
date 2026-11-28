package p

func use(...any) {}

func shapes(xs []int, ch chan int) {
	for i, v := range xs {
		go func() {
			if v > 0 {
				use(i)
			} else if v < 0 {
				use(-v)
			} else {
				use(*&i)
			}
			for j := 0; j < v; j++ {
				use(j + i)
			}
			for range xs {
				use(xs[i])
			}
			switch v {
			case 1:
				use(i)
			}
			var a any = v
			switch a.(type) {
			case int:
				use(v)
			}
			select {
			case <-ch:
				use(i)
			}
			{
				use((v))
			}
			go use(i)
			defer use(v)
			go func() { use(func() int { return i }()) }()
		}()
	}
	for i, v := range xs {
		defer func() {
			x := v
			use(x)
			use(struct{ f int }{i}.f)
		}()
	}
}

func notWalked(xs []int, ch chan int) {
	for i, v := range xs {
		go func() {
		L:
			use(i)
			i++
			ch <- v
			if x := v; x > 0 {
			}
			var y = i
			use(y)
			goto L
		}()
	}
}
