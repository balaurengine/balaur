# Plan: threads that sleep until something happens

Every native thread the engine starts should sleep in the OS until its work
arrives: a socket ready, a command queued, a deadline due. None should wake
on a timer to look. A browser build already works this way, because the page
calls back on events; this plan is about desktop and mobile.

## 0. Where the threads are today

| Thread | How it waits | Cost |
| --- | --- | --- |
| `balaur_websocket` worker, both ends of a link | a 30 ms read timeout, then the send queue | sends wait up to 30 ms; on Windows a timed-out read can lose bytes |
| `balaur_gamend` socket | its own tungstenite client: a 25 ms read timeout, 50 ms sleeps while reconnecting | a second websocket client, and the same Windows risk |
| `balaur_webtransport` outbound task | checks the engine's queue every 1 ms | a thousand wakeups a second per link |
| `balaur_http` request | one new thread and one new `ureq::Agent` per request | a thousand requests start a thousand threads and reuse no connection |
| `balaur_core::task` | one new thread per `compute` and per `step` | no bound on threads |
| DAP attach | checks every 10 ms until a debugger configures | a poll before the frame loop starts |
| Linux dark mode | asks the portal every 2 s | a poll for the life of the process |
| websocket listener | a blocking `accept` | the thread outlives the listener until one more peer arrives |

Already right: the windowed loop sleeps in `wait_events`, the frame cap sleeps
out the frame's budget, file watching takes the OS's notifications, audio's
5 ms callback runs inside the audio stream, and the CLI's one-shot jobs block
on their work.

Windows' `SO_RCVTIMEO` documentation says a receive that times out leaves the
socket in an indeterminate state that can lose data. A websocket that loses
bytes mid-frame fails, and a multiplayer guest that loses its host ends the
match: that is the likeliest cause of `a_player_leaving_is_played_as_absent`
stalling on the Windows runner.

## 1. Design

- **One `mio::Poll` per I/O thread.** The thread sleeps in `Poll::poll` until
  its socket is ready, its `mio::Waker` fires, or its next deadline is due.
  mio is epoll on Linux and Android, kqueue on macOS and iOS, and IOCP on
  Windows, and it is already in the build through tokio.
- **A queue that wakes.** `balaur_core::wake::Commands` is the engine's end
  of a thread's queue: sending a command also fires the thread's `Waker`. The
  browser build keeps a plain queue, which its per-tick pump drains.
- **Handshakes block, traffic does not.** The upgrade stays a blocking
  exchange; the socket turns non-blocking after it, keeping what arrived
  early and the TLS session's buffered plaintext.
- **Drain to `WouldBlock`.** mio reports readiness once per change, so a
  woken thread reads until the socket has nothing, and treats `WouldBlock`
  as nothing yet. A send that meets a full socket stays in tungstenite's
  buffer and goes on the next writable wakeup.
- **Deadlines are timeouts on the one wait.** A heartbeat or a reconnect
  back-off is the `Poll`'s timeout, so the thread wakes for the deadline or
  for an event, whichever comes first.
- **tokio where tokio already runs.** WebTransport's tasks take the engine's
  commands through `tokio::sync::mpsc::unbounded_channel`, whose `send` is
  synchronous and whose `recv` sleeps.
- **Bounded pools for blocking work.** HTTP requests and `compute` jobs queue
  for a fixed number of worker threads.

None of this reaches the simulation. What a tick sees is what the replay
journal says arrived before it, and wall-clock arrival already varies with
the network.

## 2. Steps

1. **Websocket on mio, built.** The worker and the listener sleep on their socket
   and waker; the 30 ms read timeout goes. The match test reports each side's
   state, tick and log when it times out. Ends with: every websocket suite
   and the multiplayer match tests pass, and an idle link takes no wakeups.
2. **Gamend on `balaur_websocket`.** Gamend's own socket code goes; the
   worker speaks Phoenix over a `balaur_websocket` connection and waits for
   the next event, command, heartbeat or back-off. Ends with: the Gamend
   suites pass against a local server, and `phoenix.rs` has no socket.
3. **WebTransport on tokio's queue.** `COMMAND_POLL` goes. Ends with: the
   WebTransport suite passes with no timed wakeup.
4. **An HTTP pool.** One shared `ureq::Agent`, so connections are kept alive
   and reused, and `[http] max_parallel` workers take requests from a queue.
   A cancel reaches a queued request too. Ends with: a thousand queued
   requests run on `max_parallel` threads and every one answers.
5. **A task pool.** `compute` runs on a pool the size of the machine's
   cores; `step` gets a small pool of its own, since its jobs are long.
   Ends with: no call in `task` starts a thread of its own.
6. **The rest.** DAP waits on its request channel with a deadline, Linux
   dark mode listens for the portal's `SettingChanged` signal, and a dropped
   listener stops its thread. Ends with: the table in section 0 is empty.
7. **Keep it that way.** `clippy.toml` disallows `thread::sleep`,
   `set_read_timeout` and `tokio::time::sleep`, so a frame cap or a real
   deadline says why in an `allow` with a reason. Ends with: clippy passes
   with every remaining sleep explained.
