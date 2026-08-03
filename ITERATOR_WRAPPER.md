Create a internal macro, which allows Generating public newtypes for internal Iterators. I want to call it like `iter_wrapper!(pub MyWrapperName<T>, MyInternal<T>)`

It should then create a Wrapper which implements Clone if MyInternal<T>: Clone and others (ImageDimension, Iterator(forward all methods not just next and size_hint, which could be optimized by parent)), if the inner implements them
