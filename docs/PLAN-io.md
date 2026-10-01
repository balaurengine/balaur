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
- **Deadlines wake the loop.** A heartbeat or a reconnect back-off stepped
  once a tick asks `balaur_core::wake::at` for a wakeup when it falls due;
  one timer thread sleeps until the earliest of them.
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
2. **Gamend on `balaur_websocket`, built.** Gamend's own socket code is gone:
   `realtime.rs` speaks Phoenix over a `balaur_websocket` `Connection` on
   every platform, stepped once a tick, and one implementation replaces the
   native thread and the browser's copy. Ends with: the Gamend suites pass
   against a local server, and `phoenix.rs` has no socket.
3. **WebTransport on tokio's queue, built.** `COMMAND_POLL` goes. A dropped
   server stops accepting, answers a newcomer with 503 while its links still
   run, and ends its thread with the last of them. Ends with: the
   WebTransport suite passes with no timed wakeup.
4. **An HTTP pool, built.** One `ureq::Agent` per engine, so connections are
   kept alive and reused, and `[http] max_parallel` workers, six unless the
   project says, take requests from a queue. A request cancelled while it
   waits never goes out. Gamend's REST calls, which its client runs one at a
   time, go to one worker per client in the order they were made. Ends with:
   requests past `max_parallel` wait their turn and every one answers.
5. **A task pool, built.** `balaur_core::task::Pool` is a queue with up to
   a limit of workers that sleep while it is empty; a job that panics costs
   no worker. `compute` and `step` each run on one a thread per core, apart,
   so a job waiting on an answer never holds the thread that computes it,
   and the HTTP pool is one too. Ends with: no call in `task` starts a
   thread of its own.
6. **The rest, built.** DAP waits on its request channel with a deadline,
   Linux dark mode sleeps on the bus until the portal's `SettingChanged`
   signal and wakes the loop when the mode flips, and a dropped listener
   stops its thread (steps 1 and 3). Ends with: every row of section 0 is
   fixed.
7. **Keep it that way, built.** The `timed-wait` house lint fails a
   `thread::sleep`, a socket read or write timeout or a `tokio::time::sleep`
   outside tests with no comment saying why. A house lint rather than
   `clippy.toml`, which cannot spare tests and their polling loops. Ends
   with: every remaining sleep outside tests is the frame cap, explained.

## 3. Not yet seen working

Linux dark mode's wait on the portal's `SettingChanged` signal is
clippy-checked on a Mac and builds on CI's Linux job, but no run on a Linux
desktop has watched it wake when the mode flips. That run retires this plan.
