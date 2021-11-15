I made a futures executor!

## What is this?

It's a future's executor. You know, like [tokio] or [async-std], it takes futures and, uh, executes them.

```rust
fn main() {
    let future = async {
        println!("In a future!");
        42
    };
    
    let runtime = guillotine::runtime::Runtime::new().unwrap();

    let _forty_two = runtime.block_on(future);
}
```

## Does it work?

Yeah! (I'm as surprised as you are.) Check out the [examples](examples) folder, where there are working examples of

* UDP sockets
* TCP sockets
* Spawning (async and blocking)
* Sleeping
* Using the `Waker` externally.


## How does it work?

The executor is built on top of `epoll` and when there is no work to do, it sits in `epoll_wait` waiting for work to do until it is woken up.

Each future is associated with an `eventfd` file descriptor that, when written to, will wake up the `epoll_wait`, giving control back to the executor. The executor then figures out which future the wakeup corresponded to and polls that future.

The `Waker` that is provided to `Future::poll` is a small wrapper around the `eventfd` and writes to the `eventfd` whenever `.wake()` is called.


## Should I use it in production?

Ha, no. Absolutely not. I created this because I wanted to better understand how futures runtimes work. Not because I thought I could make something better than what's already out there.

And in that sense, it was a success! I do feel like I understand better.

But no, this is nowhere near as good as the existing futures executors that have had countless hours poured into making them good. I still use [tokio] in production.


## Where are the interesting bits?

When I started this, I thought the most interesting bits would be the [`RawWakerVTable`]. That is in [src/runtime/waker.rs](src/runtime/waker.rs).

That turned out to be more straightforward than I was expecting – turning a pointer to an Arc into an actual Arc, and dropping or not dropping it as appropriate.

Providing built-in futures with a context that they can hook into turned out to be a bit of a challenge; setting a thread-local variable before polling a future, then clearning it afterward, so that the future can grab that context and use it. That's mostly in [src/runtime/context.rs](src/runtime/context.rs).

And because I wanted to be able to spawn futures, I found keeping track of all of the futures in the runtime was pretty interesting, too. That's in `Runtime::block` in [src/runtime/mod.rs](src/runtime/mod.rs).


[tokio]: https://docs.rs/tokio
[async-std]: https://docs.rs/async-std
[`RawWakerVTable`]: https://doc.rust-lang.org/stable/std/task/struct.RawWakerVTable.html