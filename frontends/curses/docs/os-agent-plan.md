# OS Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the rest of ADR 0019: a per-session `lem-os-agent` process that serves the desktop clipboard to Lem over `lem.os.v1`, with the Lisp channel layer shared with the display link.

**Architecture:** Lem keeps one link per peer. The display link (`lem.display.v1`, stdio) is unchanged on the wire. The agent link is new: `lem.os.v1` over a Unix domain socket the launcher names. On the Lisp side, both links sit on `lem-ipc-channels` (framing, numbering, replies, a reader thread, local-socket connect). On the Rust side, `lem-protocols` gains `os::v1`, a new `lem-os-agent` binary serves it, and the launcher starts that binary, makes the session id, and cleans up. `lem-ratatui` routes `lem-if:clipboard-*` to the agent when it has a clipboard, otherwise to the display.

**Tech Stack:** Common Lisp (SBCL), ASDF, Rove, cl-protobufs, bordeaux-threads v2 (`bt2`), `sb-bsd-sockets`; Rust 2024, prost/protox, arboard, duct, os_pipe, uuid (v7), tempfile (tests).

**Spec:** [`adr/0019-an-os-agent-and-a-link-per-peer.md`](adr/0019-an-os-agent-and-a-link-per-peer.md). Read it before starting; the plan argues from it.

## Global Constraints

- Nothing outside `frontends/ratatui/` changes: not `src/`, `extensions/`, `qlfile` or `lem.asd`.
- No `lem::`, `lem-core::` or other internal (`::`) references to Lem (`internal_symbol_rule`, `contract.yml`).
- Lisp style per `contract.yml`: first form `defpackage`/`uiop:define-package`, package named after its file; `defvar`/`defparameter` before classes before functions; `(loop :for … :do …)` with colon keywords; docstrings on exported functions, classes and generic functions.
- Session id: "a UUIDv7 in its canonical text form" (ADR 0019, 0013), made by the launcher, passed to all three processes as `LEM_SESSION_ID`.
- Socket path is passed to the agent as `--socket PATH` and to Lem as `LEM_OS_AGENT_SOCKET`.
- The agent's stdin is a pipe only the launcher holds; EOF on it means the agent removes its socket and exits.
- "The side that connects speaks first": Lem sends `Hello`, the agent answers `Welcome`.
- Clipboard deadline stays 0.1 s (`*clipboard-timeout*` in `display-relay/relay.lisp`; `*request-timeout*` for the agent).
- Protocol evolution (ADR 0010): never renumber or reuse a field; an incompatible change is a new package.
- Every existing check keeps passing: `make test`, `scripts/acceptance.py` (13 checks today, 15 after Task 8), `scripts/keyboard-acceptance.py` (8).
- Rust: `cargo clippy --all-targets` adds no warnings; `cargo fmt` on every file you touch.

## How to run things

Run from `frontends/ratatui/` unless stated.

- Rust tests: `(cd rust && cargo test)`; one crate: `(cd rust && cargo test -p lem-os-agent)`.
- Lisp tests: `make test-lisp` (all systems). One system:
  `cd ../.. && PATH="$PWD/frontends/ratatui/toolchain/bin:$PATH" .qlot/bin/rove frontends/ratatui/ipc-channels/lem-ipc-channels.asd`
- Full build: `make all`. **`sbcl` is not on PATH on this machine**; use Roswell's:
  `SB=~/.roswell/impls/x86-64/linux/sbcl-bin/2.6.8; PATH="$SB/bin:$PATH" SBCL_HOME="$SB/lib/sbcl" make all`
- Acceptance (needs `make all` first): `python3 scripts/acceptance.py`; keyboard: `(cd rust && cargo build -p lem-ratatui) && python3 scripts/keyboard-acceptance.py`.

## Where the plan departs from the ADR's wording

ADR 0019's table gives `lem-ipc-channels` "the handshake". Each link's `Hello` and `Welcome` are messages of its own package, so the channel layer cannot build them. Instead it provides everything a handshake is made of (numbered sends, replies matched by seq with a deadline, the reader thread), and each link writes its handshake in a few lines on top. `lem.os.v1`'s `Welcome` carries `reply_to` so that it is awaited like any other reply.

## Review Focus

1. **An agent that accepts but never answers.** Lem must start normally and act as if there were no agent; `connect` gives up after `*connect-timeout*` (1 s), on a background thread. Test: Task 7, `an-agent-that-never-welcomes-is-no-agent`.
2. **The agent dies mid-session.** Capabilities must drop so the clipboard falls back to the display, not hang or error on every yank. Test: Task 7, `when-the-agent-hangs-up-it-can-do-nothing`.
3. **The agent binary is missing or fails to start.** The session must run as today, with one warning printed after the terminal is restored. Tests: Task 6, `a-missing-program-is-an-error`; Task 8, acceptance check 15.
4. **A socket path that already exists.** The agent must refuse rather than take over or delete someone else's socket; each session gets a fresh private directory, removed at exit. Tests: Task 4, `refuses_a_socket_path_that_exists`; Task 5, `a_private_directory_is_owner_only_and_new`; Task 8, check 14's leftover assertion.
5. **The launcher is killed outright.** The agent must notice (stdin EOF), remove its socket and exit, so no orphan keeps running. Test: Task 4, `exits_and_removes_its_socket_when_stdin_closes`.

---

## File map

```
frontends/ratatui/
├── ipc-channels/                       NEW (Task 1)
│   ├── lem-ipc-channels.asd            lem-ipc-channels, lem-ipc-channels/tests
│   ├── framing.lisp                    moved from display-relay/protobuf/framing.lisp
│   ├── replies.lisp                    replies matched to requests, with a deadline
│   ├── channel.lisp                    channel, reader thread, local-socket connect
│   ├── main.lisp                       package lem-ipc-channels, re-exporting the three
│   └── tests/main.lisp
├── display-relay/                      MODIFIED (Task 2): onto lem-ipc-channels
│   ├── lem-display-relay.asd           depends on lem-ipc-channels; framing.lisp dropped
│   ├── relay.lisp                      clipboard replies via lem-ipc-channels
│   ├── protobuf/codec.lisp             seq leaves codec-state
│   ├── protobuf/serve.lisp             a channel instead of send-numbered
│   └── tests/{protobuf,golden}.lisp    framing tests moved out; nickname changed
├── os-agent/                           NEW (Task 7)
│   ├── lem-os-agent.asd                lem-os-agent, lem-os-agent/tests
│   ├── codec.lisp                      lem.os.v1 to and from octets
│   ├── main.lisp                       package lem-os-agent: connection and public API
│   └── tests/main.lisp
├── proto/lem/os/v1/os.proto            NEW (Task 3)
├── lisp/
│   ├── clipboard.lisp                  NEW (Task 8): agent first, display otherwise
│   └── main.lisp                       MODIFIED (Task 8): start the agent connection
├── lem-ratatui.asd                     MODIFIED (Task 8)
├── rust/
│   ├── Cargo.toml                      MODIFIED (Task 4): workspace member
│   └── crates/
│       ├── lem-protocols/              MODIFIED (Task 3): os::v1
│       ├── lem-os-agent/               NEW (Task 4)
│       │   ├── Cargo.toml
│       │   ├── src/main.rs             args, listen, lifeline, accept loop
│       │   ├── src/clipboard.rs        system (arboard) or file clipboard
│       │   ├── src/serve.rs            one connection: handshake, requests
│       │   └── tests/agent.rs          the binary, end to end
│       ├── lem-ratatui/src/main.rs     MODIFIED (Task 5): LEM_SESSION_ID
│       └── lem-ratatui-launcher/       MODIFIED (Tasks 5, 6)
│           ├── Cargo.toml              uuid; tempfile (dev)
│           ├── build.rs                third bundle payload
│           ├── src/session.rs          NEW: session id, runtime directory
│           ├── src/agent.rs            NEW: start and stop the agent
│           ├── src/bundle.rs           os_agent()
│           └── src/main.rs             wiring, env, warnings
├── scripts/
│   ├── acceptance.py                   MODIFIED (Task 8): checks 14, 15
│   └── capture-display-v1.lisp         MODIFIED (Task 2): local seq counter
├── Makefile                            MODIFIED (Tasks 1, 6, 7)
└── README.md                           MODIFIED (Task 8)
```

---

### Task 1: `lem-ipc-channels`

A new ASDF system holding what every link needs. Nothing uses it yet; Task 2 moves the display relay onto it.

**Files:**
- Create: `ipc-channels/lem-ipc-channels.asd`, `ipc-channels/framing.lisp`, `ipc-channels/replies.lisp`, `ipc-channels/channel.lisp`, `ipc-channels/main.lisp`, `ipc-channels/tests/main.lisp`
- Modify: `Makefile` (`LISP_SOURCES`, `test-lisp`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (all exported from package `lem-ipc-channels`):
  - `(write-delimited stream body)`, `(read-delimited stream)` → octet vector or `NIL` at a clean end.
  - `replies` (struct), `(make-replies)`, `(deliver-reply replies id value)`, `(await-reply replies id timeout)` → `(values value t)` or `(values nil nil)`.
  - `channel` (class), `(make-channel input output &key closer)`, `(channel-send channel encode)` → seq (integer from 1; `encode` is a function of the seq returning an octet vector, called under the channel's lock), `(channel-receive channel)` → octet vector or `NIL`, `(close-channel channel)`, `(start-reader channel handler &key name on-close)` → thread, `(connect-local path &key (timeout 1) (interval 0.02))` → channel or `NIL`, `(now-microseconds)` → integer.

- [ ] **Step 1: Write the system definition**

`ipc-channels/lem-ipc-channels.asd`:

```lisp
(defsystem "lem-ipc-channels"
  :description "Channels between Lem and its peer processes: length-delimited
framing, numbering, replies matched to requests, a reader thread, and
connecting to a local socket. ADR 0019, frontends/ratatui/docs/adr/."
  :depends-on ("alexandria" "bordeaux-threads" "log4cl" (:require "sb-bsd-sockets"))
  :serial t
  :components ((:file "framing")
               (:file "replies")
               (:file "channel")
               (:file "main"))
  :in-order-to ((test-op (test-op "lem-ipc-channels/tests"))))

(defsystem "lem-ipc-channels/tests"
  :depends-on ("lem-ipc-channels" "rove" "flexi-streams")
  :pathname "tests/"
  :components ((:file "main"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
```

- [ ] **Step 2: Write the failing tests**

`ipc-channels/tests/main.lisp`. The four framing tests are the ones in `display-relay/tests/protobuf.lisp` today, with `framing:` replaced by `ipc:`; Task 2 deletes the originals.

```lisp
(defpackage :lem-ipc-channels/tests
  (:use :cl :rove)
  (:local-nicknames (:ipc :lem-ipc-channels)))
(in-package :lem-ipc-channels/tests)

(defun octets (&rest bytes)
  (coerce bytes '(vector (unsigned-byte 8))))

(defun delimited (&rest bodies)
  (let ((out (flexi-streams:make-in-memory-output-stream)))
    (dolist (body bodies)
      (ipc:write-delimited out body))
    (flexi-streams:get-output-stream-sequence out)))

(defun undelimited (octets)
  (let ((in (flexi-streams:make-in-memory-input-stream octets)))
    (loop :for body := (ipc:read-delimited in)
          :while body
          :collect body)))

(defun temporary-socket-path ()
  (format nil "~Alem-ipc-test-~36R.sock"
          (namestring (uiop:temporary-directory))
          (random (expt 36 8) (make-random-state t))))

(defun listen-local (path)
  (let ((listener (make-instance 'sb-bsd-sockets:local-socket :type :stream)))
    (sb-bsd-sockets:socket-bind listener path)
    (sb-bsd-sockets:socket-listen listener 1)
    listener))

(defun accepted-stream (listener)
  (sb-bsd-sockets:socket-make-stream (sb-bsd-sockets:socket-accept listener)
                                     :input t :output t
                                     :element-type '(unsigned-byte 8)
                                     :buffering :full))

;;; Framing

(deftest lengths-are-varints
  (ok (equalp (octets 3 1 2 3) (delimited (octets 1 2 3))) "one byte up to 127")
  (let ((body (make-array 300 :element-type '(unsigned-byte 8) :initial-element 7)))
    (ok (equalp (octets #xAC #x02) (subseq (delimited body) 0 2)) "300 is AC 02")
    (ok (equalp (list body) (undelimited (delimited body))))))

(deftest messages-round-trip-back-to-back
  (let ((a (octets 1)) (b (octets)) (c (octets 9 9)))
    (ok (equalp (list a b c) (undelimited (delimited a b c))) "the empty one included")))

(deftest the-end-between-messages-is-not-an-error
  (ok (null (ipc:read-delimited (flexi-streams:make-in-memory-input-stream (octets))))))

(deftest the-end-inside-a-prefix-or-message-is
  (ok (signals (ipc:read-delimited (flexi-streams:make-in-memory-input-stream (octets #x80)))))
  (ok (signals (ipc:read-delimited (flexi-streams:make-in-memory-input-stream (octets 5 1))))))

;;; Replies

(deftest a-reply-delivered-before-the-wait-is-found
  (let ((replies (ipc:make-replies)))
    (ipc:deliver-reply replies 3 "three")
    (ok (equal '("three" t) (multiple-value-list (ipc:await-reply replies 3 0.1))))))

(deftest no-reply-is-nil-after-the-timeout
  (let ((replies (ipc:make-replies))
        (start (get-internal-real-time)))
    (ok (equal '(nil nil) (multiple-value-list (ipc:await-reply replies 1 0.05))))
    (ok (<= 0.05 (/ (- (get-internal-real-time) start) internal-time-units-per-second))
        "it waited out the timeout")))

(deftest a-reply-from-another-thread-wakes-the-waiter
  (let ((replies (ipc:make-replies)))
    (bt2:make-thread (lambda ()
                       (sleep 0.05)
                       (ipc:deliver-reply replies 7 :in-time)))
    (ok (eq :in-time (ipc:await-reply replies 7 2)))))

(deftest a-reply-to-an-abandoned-request-is-discarded
  (let ((replies (ipc:make-replies)))
    (ipc:deliver-reply replies 1 "stale")
    (ipc:deliver-reply replies 2 "fresh")
    (ok (equal "fresh" (ipc:await-reply replies 2 0.1)))
    (ok (null (nth-value 1 (ipc:await-reply replies 1 0.01))) "not kept for later")))

;;; Channels

(defun in-memory-channel (&optional (input (octets)))
  (let ((out (flexi-streams:make-in-memory-output-stream)))
    (values (ipc:make-channel (flexi-streams:make-in-memory-input-stream input) out)
            out)))

(deftest the-first-send-is-numbered-1
  (let ((channel (in-memory-channel)))
    (ok (= 1 (ipc:channel-send channel (lambda (seq) (octets seq)))))
    (ok (= 2 (ipc:channel-send channel (lambda (seq) (octets seq)))))))

(deftest sends-are-numbered-in-wire-order-across-threads
  (multiple-value-bind (channel out) (in-memory-channel)
    (let ((threads (loop :repeat 4
                         :collect (bt2:make-thread
                                   (lambda ()
                                     (loop :repeat 50
                                           :do (ipc:channel-send
                                                channel
                                                (lambda (seq)
                                                  (flexi-streams:string-to-octets
                                                   (princ-to-string seq))))))))))
      (mapc #'bt2:join-thread threads)
      (ok (equal (alexandria:iota 200 :start 1)
                 (mapcar (lambda (body)
                           (parse-integer (flexi-streams:octets-to-string body)))
                         (undelimited (flexi-streams:get-output-stream-sequence out))))))))

(deftest the-reader-hands-over-each-message-then-says-it-closed
  (let ((received '())
        (closed (bt2:make-semaphore))
        (channel (in-memory-channel (delimited (octets 1) (octets 2)))))
    (ipc:start-reader channel (lambda (body) (push body received))
                      :on-close (lambda () (bt2:signal-semaphore closed)))
    (ok (bt2:wait-on-semaphore closed :timeout 2))
    (ok (equalp (list (octets 1) (octets 2)) (reverse received)))))

(deftest a-handler-error-does-not-end-the-reader
  (let ((received '())
        (closed (bt2:make-semaphore))
        (channel (in-memory-channel (delimited (octets 1) (octets 2)))))
    (ipc:start-reader channel
                      (lambda (body)
                        (when (equalp body (octets 1))
                          (error "a bad message"))
                        (push body received))
                      :on-close (lambda () (bt2:signal-semaphore closed)))
    (ok (bt2:wait-on-semaphore closed :timeout 2))
    (ok (equalp (list (octets 2)) received))))

(deftest nothing-listening-is-nil-not-an-error
  (ok (null (ipc:connect-local (temporary-socket-path) :timeout 0.1))))

(deftest a-local-socket-carries-messages-both-ways
  (let* ((path (temporary-socket-path))
         (listener (listen-local path)))
    (unwind-protect
         (let ((echo (bt2:make-thread
                      (lambda ()
                        (let ((stream (accepted-stream listener)))
                          (alexandria:when-let ((body (ipc:read-delimited stream)))
                            (ipc:write-delimited stream (reverse body)))
                          (close stream)))))
               (channel (ipc:connect-local path :timeout 1)))
           (ok channel)
           (ipc:channel-send channel (lambda (seq) (declare (ignore seq)) (octets 1 2 3)))
           (ok (equalp (octets 3 2 1) (ipc:channel-receive channel)))
           (ipc:close-channel channel)
           (bt2:join-thread echo))
      (sb-bsd-sockets:socket-close listener)
      (ignore-errors (delete-file path)))))

(deftest closing-a-socket-channel-ends-its-reader
  (let* ((path (temporary-socket-path))
         (listener (listen-local path)))
    (unwind-protect
         (let ((peer nil)
               (accepted (bt2:make-semaphore))
               (closed (bt2:make-semaphore)))
           (bt2:make-thread (lambda ()
                              (setf peer (accepted-stream listener))
                              (bt2:signal-semaphore accepted)))
           (let ((channel (ipc:connect-local path :timeout 1)))
             (bt2:wait-on-semaphore accepted :timeout 2)
             (ipc:start-reader channel #'identity
                               :on-close (lambda () (bt2:signal-semaphore closed)))
             (ipc:close-channel channel)
             (ok (bt2:wait-on-semaphore closed :timeout 2)
                 "a reader blocked in read sees the end"))
           (when peer (close peer)))
      (sb-bsd-sockets:socket-close listener)
      (ignore-errors (delete-file path)))))
```

- [ ] **Step 3: Add the system to the Makefile and run the tests to see them fail**

In `Makefile`, add `$(HERE)/ipc-channels` to the `find` in `LISP_SOURCES`:

```make
LISP_SOURCES := $(HERE)/build.lisp $(HERE)/lem-ratatui.asd \
	$(shell find $(HERE)/lisp $(HERE)/display-relay $(HERE)/ipc-channels $(ROOT)/src $(ROOT)/extensions \
		-name '*.lisp' -o -name '*.asd')
```

and give `test-lisp` one rove run per system:

```make
test-lisp: $(QLOT_SETUP) $(CL_PROTOBUFS_STAMP) | $(PROTOC) $(PROTOC_PLUGIN)
	cd $(ROOT) && $(WITH_TOOLCHAIN) .qlot/bin/rove frontends/ratatui/ipc-channels/lem-ipc-channels.asd
	cd $(ROOT) && $(WITH_TOOLCHAIN) .qlot/bin/rove frontends/ratatui/display-relay/lem-display-relay.asd
```

Run: `cd ../.. && .qlot/bin/rove frontends/ratatui/ipc-channels/lem-ipc-channels.asd`
Expected: FAIL: the component files `framing`, `replies`, `channel`, `main` do not exist.

- [ ] **Step 4: Move the framing**

**Copy**, don't move: the display relay still loads the original until Task 2, which deletes it. `cp display-relay/protobuf/framing.lisp ipc-channels/framing.lisp`, then change only its package and comment:

```lisp
(defpackage :lem-ipc-channels/framing
  (:use :cl)
  (:export :write-delimited
           :read-delimited))
(in-package :lem-ipc-channels/framing)

;;; Protobuf's standard length-delimited framing, the same on every link
;;; (ADR 0019): a varint byte length, then the message. prost reads it
;;; with decode_length_delimited. Only the framing is written here; the
;;; messages themselves are cl-protobufs' to encode (ADR 0010).
```

The four functions (`write-varint`, `read-varint`, `write-delimited`, `read-delimited`) stay exactly as they are.

- [ ] **Step 5: Write `replies.lisp`**

```lisp
(defpackage :lem-ipc-channels/replies
  (:use :cl)
  (:export :replies
           :make-replies
           :deliver-reply
           :await-reply))
(in-package :lem-ipc-channels/replies)

(defstruct (replies (:constructor make-replies ()))
  "Replies to requests sent on a channel, handed from the thread that
reads the channel to the one waiting. One waiter at a time: Lem's editor
thread, which asks and blocks until the answer or a deadline.

A reply names its request by the request's seq (ADR 0013). One that
arrives for a request whose waiter has given up is discarded by the next
wait rather than mistaken for that wait's answer."
  (lock (bt2:make-lock :name "lem-ipc-channels replies"))
  (arrived (bt2:make-condition-variable :name "lem-ipc-channels reply arrived"))
  (pending '()))

(defun deliver-reply (replies id value)
  "Hand VALUE, the reply to the request sent as ID, to whoever awaits it."
  (bt2:with-lock-held ((replies-lock replies))
    (setf (replies-pending replies)
          (nconc (replies-pending replies) (list (cons id value))))
    (bt2:condition-broadcast (replies-arrived replies))))

(defun take-reply (replies id)
  "With the lock held: pop pending replies up to the one for ID, discarding
the rest. (values VALUE T) when it was there, NIL otherwise."
  (loop :for (reply-id . value) := (pop (replies-pending replies))
        :while reply-id
        :when (eql reply-id id)
          :do (return (values value t))))

(defun await-reply (replies id timeout)
  "The reply to the request sent as ID, and T; or NIL and NIL when none
arrives within TIMEOUT seconds."
  (let ((deadline (+ (get-internal-real-time)
                     (* timeout internal-time-units-per-second))))
    (bt2:with-lock-held ((replies-lock replies))
      (loop
        (multiple-value-bind (value found) (take-reply replies id)
          (when found
            (return (values value t))))
        (let ((remaining (/ (- deadline (get-internal-real-time))
                            internal-time-units-per-second)))
          (unless (plusp remaining)
            (return (values nil nil)))
          (bt2:condition-wait (replies-arrived replies) (replies-lock replies)
                              :timeout (coerce remaining 'double-float)))))))
```

- [ ] **Step 6: Write `channel.lisp`**

```lisp
(defpackage :lem-ipc-channels/channel
  (:use :cl)
  (:local-nicknames (:framing :lem-ipc-channels/framing))
  (:export :channel
           :make-channel
           :channel-send
           :channel-receive
           :close-channel
           :start-reader
           :connect-local
           :now-microseconds))
(in-package :lem-ipc-channels/channel)

(defclass channel ()
  ((input :initarg :input :reader channel-input)
   (output :initarg :output :reader channel-output)
   (lock :initform (bt2:make-lock :name "lem-ipc-channels output") :reader channel-lock)
   (seq :initform 0 :accessor channel-seq)
   (closer :initarg :closer :initform nil :reader channel-closer))
  (:documentation "One peer process over a pair of octet streams. Messages
are framed length-delimited and numbered from 1 in wire order, whichever
thread sends (ADR 0013). What a message is belongs to the link: a channel
moves octet vectors."))

(defun make-channel (input output &key closer)
  "A channel reading the octet stream INPUT and writing OUTPUT. CLOSER, a
function of no arguments, is what `close-channel' calls to release what
the streams were made from; without one, closing does nothing, as for
stdio, which the process owns."
  (make-instance 'channel :input input :output output :closer closer))

(defun channel-send (channel encode)
  "Number the next message, write what ENCODE, a function of that number,
makes of it, and return the number. ENCODE runs under the channel's lock,
so it may keep state that must follow wire order."
  (bt2:with-lock-held ((channel-lock channel))
    (let ((seq (incf (channel-seq channel))))
      (framing:write-delimited (channel-output channel) (funcall encode seq))
      seq)))

(defun channel-receive (channel)
  "The next message from the peer, an octet vector, or NIL when it hung up."
  (framing:read-delimited (channel-input channel)))

(defun close-channel (channel)
  "End CHANNEL: a reader blocked on it sees the end, and later sends fail."
  (alexandria:when-let ((closer (channel-closer channel)))
    (ignore-errors (funcall closer))))

(defun start-reader (channel handler &key (name "lem-ipc-channels reader") on-close)
  "Read CHANNEL on a new thread named NAME, calling HANDLER with each
message, until the peer hangs up or the stream fails; then call ON-CLOSE.
An error in HANDLER is logged and reading goes on: one bad message does
not end the link. Returns the thread."
  (bt2:make-thread
   (lambda ()
     (unwind-protect
          (handler-case
              (loop :for body := (channel-receive channel)
                    :while body
                    :do (handler-case (funcall handler body)
                          (error (condition)
                            (log:error "~A: failed to handle a message: ~A" name condition))))
            (error (condition)
              (log:info "~A: the stream failed: ~A" name condition)))
       (when on-close
         (funcall on-close))))
   :name name))

(defun now-microseconds ()
  "Microseconds on this process's monotonic clock (ADR 0013)."
  (floor (* (get-internal-real-time) 1000000) internal-time-units-per-second))

(defun socket-channel (socket)
  "A channel on the connected local SOCKET. One fd-stream each way on the
socket's descriptor, so the reader and the writers never share a stream;
neither closes the descriptor, which the closer does with the socket."
  (let ((fd (sb-bsd-sockets:socket-file-descriptor socket)))
    (make-channel (sb-sys:make-fd-stream fd :input t :auto-close nil
                                            :element-type '(unsigned-byte 8)
                                            :buffering :full)
                  (sb-sys:make-fd-stream fd :output t :auto-close nil
                                            :element-type '(unsigned-byte 8)
                                            :buffering :full)
                  :closer (lambda ()
                            ;; Shutting down first wakes a reader blocked in
                            ;; read(), which closing alone does not.
                            (ignore-errors (sb-bsd-sockets:socket-shutdown socket :direction :io))
                            (sb-bsd-sockets:socket-close socket)))))

(defun connect-local (path &key (timeout 1) (interval 0.02))
  "A channel to the peer listening on the local socket at PATH, or NIL when
none accepts within TIMEOUT seconds. Tried every INTERVAL seconds, since
the peer may still be starting."
  (loop :with deadline := (+ (get-internal-real-time)
                             (* timeout internal-time-units-per-second))
        :do (let ((socket (make-instance 'sb-bsd-sockets:local-socket :type :stream)))
              (handler-case
                  (progn
                    (sb-bsd-sockets:socket-connect socket (namestring path))
                    (return (socket-channel socket)))
                (sb-bsd-sockets:socket-error ()
                  (sb-bsd-sockets:socket-close socket)
                  (when (<= deadline (get-internal-real-time))
                    (return nil))
                  (sleep interval))))))
```

- [ ] **Step 7: Write `main.lisp`**

```lisp
;;;; Channels between Lem and its peer processes (ADR 0019): one package
;;;; for every link to use, made of framing, replies and channel.
(uiop:define-package :lem-ipc-channels
  (:use :cl)
  (:use-reexport :lem-ipc-channels/framing
                 :lem-ipc-channels/replies
                 :lem-ipc-channels/channel))
```

(The comment comes before the form; the first *form* is still the package definition.)

- [ ] **Step 8: Run the tests**

Run: `cd ../.. && .qlot/bin/rove frontends/ratatui/ipc-channels/lem-ipc-channels.asd`
Expected: every test passes. If `closing-a-socket-channel-ends-its-reader` hangs, the closer is not waking the reader: check that `socket-shutdown` runs before `socket-close`.

- [ ] **Step 9: Commit**

```bash
git add frontends/ratatui/ipc-channels frontends/ratatui/Makefile
git commit -m "feat(ratatui): lem-ipc-channels, what every link to Lem is made of"
```

---

### Task 2: The display relay on `lem-ipc-channels`

No behaviour change. The display link's framing, numbering and clipboard replies come from `lem-ipc-channels`; the copies in `display-relay` go.

**Files:**
- Delete: `display-relay/protobuf/framing.lisp`
- Modify: `display-relay/lem-display-relay.asd`, `display-relay/relay.lisp`, `display-relay/protobuf/codec.lisp`, `display-relay/protobuf/serve.lisp`, `display-relay/tests/protobuf.lisp`, `display-relay/tests/golden.lisp`, `scripts/capture-display-v1.lisp`, `docs/protocol.md`

**Interfaces:**
- Consumes: Task 1's `lem-ipc-channels` (all of it).
- Produces: nothing new. `codec:next-seq` is removed; `codec:make-codec-state` remains and keeps only style ids. `codec:now-microseconds` remains exported (now imported from `lem-ipc-channels`).

- [ ] **Step 1: Point the systems at `lem-ipc-channels`**

In `display-relay/lem-display-relay.asd`: `lem-display-relay` gains the dependency, and `lem-display-relay/protobuf` drops the framing component.

```lisp
  :depends-on ("lem/core" "lem-ipc-channels")
```

```lisp
  :components ((:protobuf-source-file "display"
                :proto-pathname "../../proto/lem/display/v1/display.proto")
               (:file "codec")
               (:file "serve")))
```

`git rm display-relay/protobuf/framing.lisp`.

- [ ] **Step 2: Move the framing tests out, and change the nicknames**

In `display-relay/tests/protobuf.lisp`, change the nickname `(:framing :lem-display-relay/protobuf/framing)` to `(:framing :lem-ipc-channels)` and delete the four deftests `lengths-are-varints`, `messages-round-trip-back-to-back`, `the-end-between-messages-is-not-an-error` and `the-end-inside-a-prefix-or-message-is` (they are in Task 1's suite now). Keep the helpers `delimited`, `undelimited` and `octets`: later tests use them.

In `display-relay/tests/golden.lisp`, change `(:framing :lem-display-relay/protobuf/framing)` to `(:framing :lem-ipc-channels)`.

- [ ] **Step 3: Run the display tests to see them fail**

Run: `make test-lisp`
Expected: `lem-display-relay/tests` fails to load: `serve.lisp` still refers to `lem-display-relay/protobuf/framing`.

- [ ] **Step 4: Clipboard replies in `relay.lisp`**

In the `defpackage`, replace `(:queue :lem/common/queue)` with `(:ipc :lem-ipc-channels)`. Confirm `queue:` has no other use in the file (`grep -n "queue:" display-relay/relay.lisp` lists only the lines below).

Slot:

```lisp
   (clipboard-replies :initform (ipc:make-replies)
                      :reader relay-clipboard-replies))
```

Functions:

```lisp
(defun await-clipboard (relay id)
  "The text replied to the request sent as ID, or NIL after
`*clipboard-timeout*'. A late reply to an earlier request is discarded,
not mistaken for this one's."
  (values (ipc:await-reply (relay-clipboard-replies relay) id *clipboard-timeout*)))

(defun clipboard-replied (relay reply-to text)
  "Hand the display's clipboard TEXT, answering the request sent as
REPLY-TO, to the `lem-if:clipboard-paste' waiting for it.

Called from the thread reading the display, never the editor's: the
editor thread is blocked in `lem-if:clipboard-paste' waiting for exactly
this, so the reply cannot go through `lem:send-event' (ADR 0012)."
  (ipc:deliver-reply (relay-clipboard-replies relay) reply-to text))
```

- [ ] **Step 5: The seq leaves the codec**

In `display-relay/protobuf/codec.lisp`:

- In the `defpackage`, add `(:import-from :lem-ipc-channels :now-microseconds)` and remove `:next-seq` from `:export` (keep `:now-microseconds` exported).
- Delete the `next-seq` and `now-microseconds` functions.
- `codec-state` keeps only the styles:

```lisp
(defstruct (codec-state (:constructor make-codec-state ()))
  "What the codec remembers across messages: the styles interned so far.

Style ids are the codec's (ADR 0012): the model carries styles as values,
and the codec gives each distinct one an id the first time a frame uses
it, sending its definition in that frame. Ids are stable for the
session; 0 is no style. Messages are numbered by the channel, which calls
`encode' under its lock, so interning follows wire order."
  (style-ids (make-hash-table :test 'equal))
  (last-style-id 0))
```

- [ ] **Step 6: `serve.lisp` on a channel**

Replace the whole file:

```lisp
(defpackage :lem-display-relay/protobuf/serve
  (:use :cl)
  (:local-nicknames (:ipc :lem-ipc-channels)
                    (:codec :lem-display-relay/protobuf/codec)
                    (:relay :lem-display-relay/relay)
                    (:input :lem-display-relay/input))
  (:export :serve))
(in-package :lem-display-relay/protobuf/serve)

(defun unpack-color (rgb)
  (and rgb (lem:make-color (ldb (byte 8 16) rgb) (ldb (byte 8 8) rgb) (ldb (byte 8 0) rgb))))

(defun hello (relay channel ready version session-id width height foreground background
              capabilities)
  "Take the display's `Hello': its size, colours and capabilities, then `Welcome', then
let the editor start (see `serve'). Return NIL when the display cannot
be served, having told it why with `Exit'."
  (when (zerop version)
    (ipc:channel-send channel
                      (lambda (seq)
                        (codec:exit-message seq "Hello carried no protocol_version.")))
    (return-from hello nil))
  (log:info "lem-display-relay/protobuf: session ~A, display protocol revision ~D, terminal colours ~A on ~A"
            session-id version
            (if foreground (format nil "#~6,'0X" foreground) "unknown")
            (if background (format nil "#~6,'0X" background) "unknown"))
  (when (and (plusp width) (plusp height))
    (relay:set-display-size relay width height))
  ;; A reader-only slot: the codec sets handshake state before the editor
  ;; starts. User configuration observes an immutable value after this.
  (reinitialize-instance relay :terminal-capabilities capabilities)
  ;; The terminal's own colours: what Lem judges light or dark mode by
  ;; until a theme sets its own.
  (alexandria:when-let ((color (unpack-color foreground)))
    (setf (relay:relay-foreground relay) color))
  (alexandria:when-let ((color (unpack-color background)))
    (setf (relay:relay-background relay) color))
  (ipc:channel-send channel (lambda (seq) (codec:welcome seq session-id)))
  (bt2:signal-semaphore ready)
  t)

(defun serve (relay function &key input output)
  "Run Lem with RELAY, speaking lem.display.v1 (display.proto) on the octet
streams INPUT and OUTPUT, until the editor exits or the display hangs up.
The body of an implementation's `lem-if:invoke'; FUNCTION is what Lem
hands that. Returns the editor's crash report, if it left one.

As ncurses does (frontends/ncurses/mainloop.lisp): this thread reads,
the editor runs on its own thread, and the editor's finalize callback
ends the loop. The editor waits for `Hello', which carries the screen's
size, and sends its first frame unasked: there is no `redraw' step
(ADR 0011). When the editor exits, the display gets `Exit' before the
stream ends. Writes from both threads go through one channel, which
serialises and numbers them (ADR 0019)."
  (let ((channel (ipc:make-channel input output))
        (ready (bt2:make-semaphore :name "lem-display-relay/protobuf hello"))
        (state (codec:make-codec-state))
        (reader (bt2:current-thread)))
    (setf (relay:relay-sink relay)
          (lambda (message)
            (ipc:channel-send channel (lambda (seq) (codec:encode message state seq)))))
    (catch 'editor-exited
      (setf (relay:relay-editor-thread relay)
            (funcall function
                     (lambda () (bt2:wait-on-semaphore ready))
                     (lambda (report)
                       (ignore-errors
                        (ipc:channel-send channel
                                          (lambda (seq)
                                            (codec:exit-message seq (or report "")))))
                       (bt2:interrupt-thread reader
                                             (lambda () (throw 'editor-exited report))))))
      (loop :for body := (ipc:channel-receive channel)
            :while body
            :do (handler-case
                    (let ((request (codec:decode body)))
                      (ecase (first request)
                        (:hello
                         (unless (apply #'hello relay channel ready (cddr request))
                           (return)))
                        (:input (input:deliver relay (second request)))
                        (:ignored
                         (log:info "lem-display-relay/protobuf: ignoring a message this revision does not know"))))
                  (error (condition)
                    (log:error "lem-display-relay/protobuf: failed to handle a message: ~A" condition))))
      nil)))
```

- [ ] **Step 7: The capture script numbers its own messages**

In `scripts/capture-display-v1.lisp`, `capture` used `codec:next-seq`. Replace its `let` and sink with a local counter, and `lem-display-relay/protobuf/framing:write-delimited` (in the fixture writer) with `lem-ipc-channels:write-delimited`:

```lisp
  (let ((relay (make-instance 'capture-relay))
        (state (lem-display-relay/protobuf/codec:make-codec-state))
        (seq 0)
        (bodies '()))
    (setf (lem-display-relay/relay:relay-sink relay)
          (lambda (message)
            (incf seq)
            (push (lem-display-relay/protobuf/codec:encode message state seq) bodies)
            seq))
```

- [ ] **Step 8: `docs/protocol.md`**

In the table under the framing paragraph, the Lisp framing is now `ipc-channels/framing.lisp`:

```markdown
| framing | `ipc-channels/framing.lisp` | `lem_protocols::framing::{read,write}_delimited` |
```

- [ ] **Step 9: Run everything that touches the display link**

Run: `make test-lisp`
Expected: `lem-ipc-channels/tests` and `lem-display-relay/tests` pass; the display suite has 4 fewer framing tests than before.

Run: `grep -rn "protobuf/framing\|next-seq\|send-numbered\|lem/common/queue" display-relay scripts lisp`
Expected: no output.

Run the full build and both acceptance scripts (see *How to run things*).
Expected: 13/13 and 8/8.

- [ ] **Step 10: Commit**

```bash
git add -A frontends/ratatui/display-relay frontends/ratatui/scripts/capture-display-v1.lisp frontends/ratatui/docs/protocol.md
git commit -m "refactor(ratatui): the display relay on lem-ipc-channels"
```

---

### Task 3: The `lem.os.v1` schema, and `lem_protocols::os::v1`

**Files:**
- Create: `proto/lem/os/v1/os.proto`, `rust/crates/lem-protocols/src/os.rs`, `rust/crates/lem-protocols/src/os/v1.rs`
- Modify: `rust/crates/lem-protocols/build.rs`, `rust/crates/lem-protocols/src/lib.rs`, `rust/crates/lem-protocols/Cargo.toml` (description)

**Interfaces:**
- Produces, for Rust (`lem_protocols::os::v1`): `ToAgent { seq, time_us, message: Option<to_agent::Message> }` with `to_agent::Message::{Hello, ClipboardRequest, SetClipboard}`; `ToEditor { seq, time_us, message: Option<to_editor::Message> }` with `to_editor::Message::{Welcome, ClipboardReply}`; `Hello { protocol_version, session_id }`; `Welcome { protocol_version, session_id, capabilities: Option<Capabilities>, reply_to }`; `Capabilities { clipboard: bool }`; `ClipboardRequest {}`; `ClipboardReply { reply_to, text: Option<String> }`; `SetClipboard { text }`; `PROTOCOL_VERSION: u32 = 1`.
- Produces, for Lisp (Task 7): package `cl-protobufs.lem.os.v1` with the same messages (`pb:make-to-agent`, `pb:to-editor.message-case`, …).

- [ ] **Step 1: Write the schema**

`proto/lem/os/v1/os.proto`:

```proto
// The protocol between Lem and the OS agent: lem-os-agent in Lisp on the
// editor side, the lem-os-agent process on the other.
//
// Decisions behind it, in frontends/ratatui/docs/adr/:
//   0019  an OS agent, and a link per peer
// It follows display.proto's conventions (ADR 0011, 0013): every message
// carries its sender's seq and time; a reply names its request by seq.
//
// Transport: a local (Unix domain) stream socket. The launcher picks its
// path, in a directory private to the session, and gives it to the agent
// as `--socket PATH` and to Lem as LEM_OS_AGENT_SOCKET. Lem connects, so
// Lem speaks first: Hello, answered by Welcome. Each message is one
// ToAgent or ToEditor, framed as on the display link: a varint byte
// length, then the message.
//
// Evolution: never renumber or reuse a field; `reserved` what is removed.
// An incompatible change is package lem.os.v2, not an edit to v1. A new
// feature comes with a capability, so that an older agent says it lacks it.

syntax = "proto3";

package lem.os.v1;

// ---------------------------------------------------------------------------
// Envelopes

// Everything Lem sends.
message ToAgent {
  // 1 for Lem's first message on this connection, one more for each after.
  uint64 seq = 1;
  // Microseconds on Lem's monotonic clock since it started.
  uint64 time_us = 2;

  oneof message {
    Hello hello = 3;
    ClipboardRequest clipboard_request = 4;
    SetClipboard set_clipboard = 5;
  }
}

// Everything the agent sends.
message ToEditor {
  // The agent's own numbering, as ToAgent.seq.
  uint64 seq = 1;
  // Microseconds on the agent's monotonic clock since it started.
  uint64 time_us = 2;

  oneof message {
    Welcome welcome = 3;
    ClipboardReply clipboard_reply = 4;
  }
}

// ---------------------------------------------------------------------------
// Handshake

message Hello {
  // Revision of this v1 schema Lem was built against; never 0.
  uint32 protocol_version = 1;
  // The session's id, LEM_SESSION_ID: a UUIDv7 in canonical text form,
  // made by the launcher. Empty when Lem was started without one.
  string session_id = 2;
}

message Welcome {
  // The agent's revision of this schema.
  uint32 protocol_version = 1;
  // Hello.session_id, echoed.
  string session_id = 2;
  // What this agent can do on this desktop. Absent means nothing.
  Capabilities capabilities = 3;
  // The Hello's ToAgent.seq.
  uint64 reply_to = 4;
}

// What the agent offers. False for anything this desktop lacks: no
// display server, no clipboard.
message Capabilities {
  bool clipboard = 1;
}

// ---------------------------------------------------------------------------
// Clipboard. Lem asks when it yanks and waits briefly (0.1 s) for the
// answer; the reply names the request by its seq.

message ClipboardRequest {}

message ClipboardReply {
  // The ClipboardRequest's ToAgent.seq.
  uint64 reply_to = 1;
  // Absent when there is nothing readable on the clipboard.
  optional string text = 2;
}

message SetClipboard {
  string text = 1;
}
```

- [ ] **Step 2: Write the failing Rust test**

`rust/crates/lem-protocols/src/os/v1.rs`:

```rust
//! `lem.os.v1`: the protocol between Lem and the OS agent (ADR 0019).
//!
//! The types are generated from
//! `frontends/ratatui/proto/lem/os/v1/os.proto` by `build.rs`; the schema,
//! with its comments, is the reference. Framing on a stream is
//! [`crate::framing`], shared by every link.

mod generated {
    // Generated code: its style is prost's, not ours.
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/lem.os.v1.rs"));
}
pub use generated::*;

/// The revision of this schema this crate was built against (`Hello`).
pub const PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn a_welcome_round_trips() {
        let welcome = ToEditor {
            seq: 1,
            time_us: 5,
            message: Some(to_editor::Message::Welcome(Welcome {
                protocol_version: PROTOCOL_VERSION,
                session_id: "0190a0d6-0000-7000-8000-000000000000".into(),
                capabilities: Some(Capabilities { clipboard: true }),
                reply_to: 1,
            })),
        };
        let bytes = welcome.encode_to_vec();
        assert_eq!(ToEditor::decode(bytes.as_slice()).unwrap(), welcome);
    }

    #[test]
    fn an_empty_clipboard_is_absent_text_not_empty_text() {
        let reply = ClipboardReply {
            reply_to: 2,
            text: None,
        };
        let decoded = ClipboardReply::decode(reply.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.text, None);
    }
}
```

`rust/crates/lem-protocols/src/os.rs`:

```rust
//! The link between Lem and the OS agent: `lem.os.v1`.

pub mod v1;
```

In `src/lib.rs`, add the module and a doc line:

```rust
//! - [`os::v1`] is `lem.os.v1`, generated from
//!   `frontends/ratatui/proto/lem/os/v1/os.proto` (ADR 0019); the other
//!   side of the wire is `frontends/ratatui/os-agent/`.
```

```rust
pub mod display;
pub mod framing;
pub mod os;
```

Run: `(cd rust && cargo test -p lem-protocols)`
Expected: FAIL to compile: `lem.os.v1.rs` was not generated.

- [ ] **Step 3: Generate it**

In `rust/crates/lem-protocols/build.rs`:

```rust
const SCHEMAS: &[&str] = &["lem/display/v1/display.proto", "lem/os/v1/os.proto"];
```

In `rust/crates/lem-protocols/Cargo.toml`:

```toml
description = "Types and framing for Lem's protocols, one per link: lem.display.v1 with a display, lem.os.v1 with the OS agent."
```

- [ ] **Step 4: Run the tests**

Run: `(cd rust && cargo test -p lem-protocols && cargo clippy -p lem-protocols --all-targets)`
Expected: all pass (the 2 new ones included), no new warnings.

- [ ] **Step 5: Commit**

```bash
git add frontends/ratatui/proto/lem/os frontends/ratatui/rust/crates/lem-protocols
git commit -m "feat(ratatui): lem.os.v1, the schema between Lem and the OS agent"
```

---

### Task 4: The `lem-os-agent` binary

**Files:**
- Create: `rust/crates/lem-os-agent/Cargo.toml`, `src/main.rs`, `src/clipboard.rs`, `src/serve.rs`, `tests/agent.rs`
- Modify: `rust/Cargo.toml` (members)

**Interfaces:**
- Consumes: Task 3's `lem_protocols::os::v1`, and `lem_protocols::framing::{read_delimited, write_delimited}`.
- Produces: a binary `lem-os-agent` run as `lem-os-agent --socket PATH`. Reads `LEM_SESSION_ID` (for its log) and `LEM_OS_AGENT_CLIPBOARD_FILE` (when set, that file is the clipboard: for tests, and for trying the agent without a desktop). Exits 0 when stdin reaches EOF, after removing the socket; exits 1 when it cannot listen (including when PATH exists); exits 2 on bad arguments.

- [ ] **Step 1: The crate**

`rust/crates/lem-os-agent/Cargo.toml`:

```toml
[package]
name = "lem-os-agent"
description = "Lem's OS agent: the desktop's clipboard, and later its tray and notifications, for one editor session over lem.os.v1."
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[lints]
workspace = true

[dependencies]
anyhow = "1.0.104"
arboard = { version = "3.6.1", default-features = false }
lem-protocols = { path = "../lem-protocols" }
prost = "0.14"

[dev-dependencies]
tempfile = "3"
```

In `rust/Cargo.toml`:

```toml
members = ["crates/lem-protocols", "crates/lem-os-agent", "crates/lem-ratatui", "crates/lem-ratatui-launcher"]
```

- [ ] **Step 2: The clipboard backends**

`src/clipboard.rs`:

```rust
//! The clipboard the agent offers: the desktop's, or a file standing in
//! for it.

use std::path::PathBuf;

/// Somewhere text can be copied to and pasted from.
pub trait Clipboard {
    /// The text on it, or `None` when there is nothing readable.
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// The desktop's clipboard, through `arboard` (X11; as the display's).
struct System(arboard::Clipboard);

impl Clipboard for System {
    fn get(&mut self) -> Option<String> {
        self.0.get_text().ok()
    }

    fn set(&mut self, text: &str) {
        let _ = self.0.set_text(text);
    }
}

/// A file as the clipboard: `LEM_OS_AGENT_CLIPBOARD_FILE`. For tests, and
/// for trying the agent on a machine with no desktop.
struct File(PathBuf);

impl Clipboard for File {
    fn get(&mut self) -> Option<String> {
        std::fs::read_to_string(&self.0).ok()
    }

    fn set(&mut self, text: &str) {
        let _ = std::fs::write(&self.0, text);
    }
}

/// The clipboard to offer, or `None` when there is none: no display
/// server, as over SSH or in a container. That is not an error; the
/// agent says so in `Welcome` and Lem uses the display's instead.
pub fn select() -> Option<Box<dyn Clipboard>> {
    if let Some(path) = std::env::var_os("LEM_OS_AGENT_CLIPBOARD_FILE").filter(|p| !p.is_empty()) {
        return Some(Box::new(File(PathBuf::from(path))));
    }
    match arboard::Clipboard::new() {
        Ok(clipboard) => Some(Box::new(System(clipboard))),
        Err(error) => {
            eprintln!("lem-os-agent: no system clipboard ({error})");
            None
        }
    }
}
```

- [ ] **Step 3: Write the failing `serve` tests**

`src/serve.rs`, tests first (the function bodies come in Step 4):

```rust
//! One connection from Lem: the handshake, then its requests until it
//! hangs up.

use std::io::{self, Read, Write};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use lem_protocols::framing;
use lem_protocols::os::v1::{self, to_agent, to_editor};
use prost::Message;

use crate::clipboard::Clipboard;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// A clipboard the test can look into afterwards.
    struct Memory(Rc<RefCell<Option<String>>>);

    impl Clipboard for Memory {
        fn get(&mut self) -> Option<String> {
            self.0.borrow().clone()
        }
        fn set(&mut self, text: &str) {
            *self.0.borrow_mut() = Some(text.to_owned());
        }
    }

    fn memory(text: Option<&str>) -> (Option<Box<dyn Clipboard>>, Rc<RefCell<Option<String>>>) {
        let cell = Rc::new(RefCell::new(text.map(str::to_owned)));
        (Some(Box::new(Memory(cell.clone()))), cell)
    }

    fn wire(messages: &[(u64, to_agent::Message)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (seq, message) in messages {
            let envelope = v1::ToAgent {
                seq: *seq,
                time_us: 0,
                message: Some(message.clone()),
            };
            framing::write_delimited(&mut out, &envelope).unwrap();
        }
        out
    }

    fn sent(out: &[u8]) -> Vec<v1::ToEditor> {
        let mut stream = out;
        let mut messages = Vec::new();
        while let Some(body) = framing::read_delimited(&mut stream).unwrap() {
            messages.push(v1::ToEditor::decode(body.as_slice()).unwrap());
        }
        messages
    }

    fn hello(version: u32) -> to_agent::Message {
        to_agent::Message::Hello(v1::Hello {
            protocol_version: version,
            session_id: "s".into(),
        })
    }

    fn welcome(message: &v1::ToEditor) -> &v1::Welcome {
        match &message.message {
            Some(to_editor::Message::Welcome(welcome)) => welcome,
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn welcomes_lem_with_what_it_can_do() {
        let (mut clipboard, _) = memory(None);
        let mut out = Vec::new();
        let session = serve(wire(&[(1, hello(1))]).as_slice(), &mut out, &mut clipboard).unwrap();
        assert_eq!(session, "s");
        let sent = sent(&out);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].seq, 1);
        let welcome = welcome(&sent[0]);
        assert_eq!(welcome.protocol_version, v1::PROTOCOL_VERSION);
        assert_eq!(welcome.session_id, "s");
        assert_eq!(welcome.reply_to, 1);
        assert_eq!(welcome.capabilities, Some(v1::Capabilities { clipboard: true }));
    }

    #[test]
    fn without_a_clipboard_it_says_so() {
        let mut out = Vec::new();
        serve(wire(&[(1, hello(1))]).as_slice(), &mut out, &mut None).unwrap();
        assert_eq!(
            welcome(&sent(&out)[0]).capabilities,
            Some(v1::Capabilities { clipboard: false })
        );
    }

    #[test]
    fn answers_a_clipboard_request_by_its_seq() {
        let (mut clipboard, _) = memory(Some("pasted"));
        let mut out = Vec::new();
        let input = wire(&[
            (1, hello(1)),
            (2, to_agent::Message::ClipboardRequest(v1::ClipboardRequest {})),
        ]);
        serve(input.as_slice(), &mut out, &mut clipboard).unwrap();
        let sent = sent(&out);
        assert_eq!(sent[1].seq, 2, "numbered in order");
        assert_eq!(
            sent[1].message,
            Some(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                reply_to: 2,
                text: Some("pasted".into()),
            }))
        );
    }

    #[test]
    fn a_request_without_a_clipboard_gets_no_text() {
        let mut out = Vec::new();
        let input = wire(&[
            (1, hello(1)),
            (2, to_agent::Message::ClipboardRequest(v1::ClipboardRequest {})),
        ]);
        serve(input.as_slice(), &mut out, &mut None).unwrap();
        assert_eq!(
            sent(&out)[1].message,
            Some(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                reply_to: 2,
                text: None,
            }))
        );
    }

    #[test]
    fn sets_the_clipboard() {
        let (mut clipboard, cell) = memory(None);
        let input = wire(&[
            (1, hello(1)),
            (2, to_agent::Message::SetClipboard(v1::SetClipboard { text: "copied".into() })),
        ]);
        serve(input.as_slice(), &mut Vec::new(), &mut clipboard).unwrap();
        assert_eq!(cell.borrow().as_deref(), Some("copied"));
    }

    #[test]
    fn the_first_message_must_be_hello() {
        let input = wire(&[(1, to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}))]);
        let mut out = Vec::new();
        assert!(serve(input.as_slice(), &mut out, &mut None).is_err());
        assert!(out.is_empty(), "nothing sent to a peer that did not say Hello");
    }

    #[test]
    fn a_hello_without_a_protocol_version_is_refused() {
        let mut out = Vec::new();
        assert!(serve(wire(&[(1, hello(0))]).as_slice(), &mut out, &mut None).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn hanging_up_before_hello_is_an_error() {
        assert!(serve(&[][..], &mut Vec::new(), &mut None).is_err());
    }
}
```

Add a placeholder-free stub so the crate compiles for this step: `src/main.rs` with only

```rust
mod clipboard;
mod serve;

fn main() {}
```

Run: `(cd rust && cargo test -p lem-os-agent)`
Expected: FAIL to compile: `serve` is not defined.

- [ ] **Step 4: Write `serve`**

Above the tests in `src/serve.rs`:

```rust
/// Numbers and times what the agent sends, as ADR 0013 asks of every link.
struct Writer<W> {
    out: W,
    seq: u64,
    started: Instant,
}

impl<W: Write> Writer<W> {
    fn new(out: W) -> Self {
        Self {
            out,
            seq: 0,
            started: Instant::now(),
        }
    }

    fn send(&mut self, message: to_editor::Message) -> io::Result<()> {
        self.seq += 1;
        let envelope = v1::ToEditor {
            seq: self.seq,
            time_us: u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX),
            message: Some(message),
        };
        framing::write_delimited(&mut self.out, &envelope)
    }
}

fn read(input: &mut impl Read) -> Result<Option<v1::ToAgent>> {
    let Some(body) = framing::read_delimited(input)? else {
        return Ok(None);
    };
    Ok(Some(
        v1::ToAgent::decode(body.as_slice()).context("decoding a ToAgent")?,
    ))
}

/// Serve one Lem, reading `input` and writing `output`, until it hangs up.
/// Returns the session id its `Hello` carried.
///
/// The first message must be a `Hello` with a protocol version; anything
/// else ends the connection unanswered. A message this revision does not
/// know decodes with no `message` and is ignored, as a second `Hello` is.
pub fn serve(
    mut input: impl Read,
    output: impl Write,
    clipboard: &mut Option<Box<dyn Clipboard>>,
) -> Result<String> {
    let mut writer = Writer::new(output);
    let first = read(&mut input)?.context("Lem hung up before Hello")?;
    let Some(to_agent::Message::Hello(hello)) = first.message else {
        bail!("the first message was not Hello");
    };
    if hello.protocol_version == 0 {
        bail!("Hello carried no protocol_version");
    }
    writer.send(to_editor::Message::Welcome(v1::Welcome {
        protocol_version: v1::PROTOCOL_VERSION,
        session_id: hello.session_id.clone(),
        capabilities: Some(v1::Capabilities {
            clipboard: clipboard.is_some(),
        }),
        reply_to: first.seq,
    }))?;

    while let Some(envelope) = read(&mut input)? {
        match envelope.message {
            Some(to_agent::Message::ClipboardRequest(_)) => {
                let text = clipboard.as_mut().and_then(|clipboard| clipboard.get());
                writer.send(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                    reply_to: envelope.seq,
                    text,
                }))?;
            }
            Some(to_agent::Message::SetClipboard(set)) => {
                if let Some(clipboard) = clipboard.as_mut() {
                    clipboard.set(&set.text);
                }
            }
            Some(to_agent::Message::Hello(_)) | None => {}
        }
    }
    Ok(hello.session_id)
}
```

Run: `(cd rust && cargo test -p lem-os-agent)`
Expected: the 8 `serve` tests pass.

- [ ] **Step 5: Write the failing end-to-end tests**

`tests/agent.rs`:

```rust
//! The agent as the launcher runs it: a socket to listen on, a pipe on
//! stdin, and the clipboard a file (`LEM_OS_AGENT_CLIPBOARD_FILE`).

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use lem_protocols::framing;
use lem_protocols::os::v1::{self, to_agent, to_editor};
use prost::Message;

struct Agent {
    child: Child,
    socket: PathBuf,
    /// Held so the directory outlives the agent.
    _dir: tempfile::TempDir,
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(clipboard_file: Option<&Path>) -> Agent {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("os-agent.sock");
    let mut command = Command::new(env!("CARGO_BIN_EXE_lem-os-agent"));
    command
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::piped())
        .env("LEM_SESSION_ID", "test-session");
    match clipboard_file {
        Some(file) => command.env("LEM_OS_AGENT_CLIPBOARD_FILE", file),
        None => command.env_remove("LEM_OS_AGENT_CLIPBOARD_FILE"),
    };
    let child = command.spawn().unwrap();
    Agent {
        child,
        socket,
        _dir: dir,
    }
}

fn connect(socket: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(socket) {
            Ok(stream) => return stream,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("connecting to {}: {error}", socket.display()),
        }
    }
}

fn send(stream: &mut UnixStream, seq: u64, message: to_agent::Message) {
    let envelope = v1::ToAgent {
        seq,
        time_us: 0,
        message: Some(message),
    };
    framing::write_delimited(stream, &envelope).unwrap();
}

fn receive(stream: &mut UnixStream) -> to_editor::Message {
    let body = framing::read_delimited(stream).unwrap().expect("a message");
    v1::ToEditor::decode(body.as_slice()).unwrap().message.expect("a known message")
}

fn hello(stream: &mut UnixStream) -> v1::Welcome {
    send(
        stream,
        1,
        to_agent::Message::Hello(v1::Hello {
            protocol_version: v1::PROTOCOL_VERSION,
            session_id: "test-session".into(),
        }),
    );
    match receive(stream) {
        to_editor::Message::Welcome(welcome) => welcome,
        other => panic!("expected Welcome, got {other:?}"),
    }
}

fn wait(child: &mut Child, within: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn welcomes_lem_and_offers_the_file_clipboard() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let agent = start(Some(file.path()));
    let welcome = hello(&mut connect(&agent.socket));
    assert_eq!(welcome.session_id, "test-session");
    assert_eq!(welcome.reply_to, 1);
    assert_eq!(welcome.capabilities, Some(v1::Capabilities { clipboard: true }));
}

#[test]
fn pastes_and_copies_through_the_clipboard() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "on the clipboard").unwrap();
    let agent = start(Some(file.path()));
    let mut stream = connect(&agent.socket);
    hello(&mut stream);

    send(&mut stream, 2, to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}));
    assert_eq!(
        receive(&mut stream),
        to_editor::Message::ClipboardReply(v1::ClipboardReply {
            reply_to: 2,
            text: Some("on the clipboard".into()),
        })
    );

    send(&mut stream, 3, to_agent::Message::SetClipboard(v1::SetClipboard { text: "copied".into() }));
    // The agent does not answer SetClipboard; a request after it is
    // answered only once the set is done, so asking again orders them.
    send(&mut stream, 4, to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}));
    receive(&mut stream);
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "copied");
}

#[test]
fn exits_and_removes_its_socket_when_stdin_closes() {
    let mut agent = start(None);
    drop(connect(&agent.socket));
    drop(agent.child.stdin.take());
    let status = wait(&mut agent.child, Duration::from_secs(5)).expect("exited");
    assert!(status.success(), "{status}");
    assert!(!agent.socket.exists(), "socket removed");
}

#[test]
fn refuses_a_socket_path_that_exists() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("taken");
    std::fs::write(&socket, "").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lem-os-agent"))
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    assert!(socket.exists(), "not removed: it was not the agent's");
}

#[test]
fn bad_arguments_are_a_usage_error() {
    let status = Command::new(env!("CARGO_BIN_EXE_lem-os-agent"))
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
```

Run: `(cd rust && cargo test -p lem-os-agent --test agent)`
Expected: FAIL: the stub `main` does nothing, so connecting times out and the exit codes are wrong.

- [ ] **Step 6: Write `main.rs`**

```rust
//! Lem's OS agent (ADR 0019): what the desktop offers an editor, over
//! `lem.os.v1` on a local socket.
//!
//! The launcher starts it with `--socket PATH` and a pipe on its stdin
//! that only the launcher holds. When that pipe closes — the session is
//! over, or the launcher died — the agent removes its socket and exits.
//! It serves one Lem at a time: an agent belongs to one session.

mod clipboard;
mod serve;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let socket = match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None) if flag == "--socket" => PathBuf::from(path),
        _ => {
            eprintln!("usage: lem-os-agent --socket PATH");
            return ExitCode::from(2);
        }
    };
    match run(&socket) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lem-os-agent: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
fn run(socket: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::net::UnixListener;

    use anyhow::Context;

    // Binding fails when the path exists: a socket is never taken over.
    let listener = UnixListener::bind(socket)
        .with_context(|| format!("listening on {}", socket.display()))?;
    watch_lifeline(socket.to_path_buf());
    let session = std::env::var("LEM_SESSION_ID").unwrap_or_default();
    let mut clipboard = clipboard::select();
    for stream in listener.incoming() {
        let stream = stream.context("accepting a connection")?;
        let reader = stream.try_clone().context("cloning the connection")?;
        match serve::serve(reader, stream, &mut clipboard) {
            Ok(_) => eprintln!("lem-os-agent: session {session}: Lem hung up"),
            Err(error) => eprintln!("lem-os-agent: session {session}: {error:#}"),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn run(_socket: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("only Unix is supported")
}

/// Exit when stdin closes, removing the socket first.
#[cfg(unix)]
fn watch_lifeline(socket: PathBuf) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = std::fs::remove_file(&socket);
        std::process::exit(0);
    });
}
```

- [ ] **Step 7: Run all the agent's tests**

Run: `(cd rust && cargo test -p lem-os-agent && cargo clippy -p lem-os-agent --all-targets && cargo fmt -p lem-os-agent --check)`
Expected: 8 unit and 5 end-to-end tests pass; no warnings; no formatting diff.

- [ ] **Step 8: Commit**

```bash
git add frontends/ratatui/rust/Cargo.toml frontends/ratatui/rust/Cargo.lock frontends/ratatui/rust/crates/lem-os-agent
git commit -m "feat(ratatui): lem-os-agent, the desktop's clipboard over lem.os.v1"
```

---

### Task 5: One session id, made by the launcher

**Files:**
- Create: `rust/crates/lem-ratatui-launcher/src/session.rs`
- Modify: `rust/crates/lem-ratatui-launcher/Cargo.toml`, `rust/crates/lem-ratatui-launcher/src/main.rs`, `rust/crates/lem-ratatui/src/main.rs`

**Interfaces:**
- Produces (launcher, module `session`): `new_id() -> String`; `runtime_dir(xdg_runtime_dir: Option<OsString>, temp_dir: &Path, id: &str) -> PathBuf`; `create_private(dir: &Path) -> io::Result<()>`. Task 6 uses all three.
- Produces (display): `session_id(from_launcher: Option<String>) -> String`.

- [ ] **Step 1: Dependencies**

`rust/crates/lem-ratatui-launcher/Cargo.toml`:

```toml
[dependencies]
anyhow = "1.0.104"
duct = "1.1.2"
os_pipe = "1.2.3"
uuid = { version = "1", features = ["v7"] }
zstd = { version = "0.14.0", optional = true }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write `session.rs` with its tests failing**

```rust
//! What identifies this session to its processes, and where it keeps its
//! sockets (ADR 0019).

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A new session id: a UUIDv7 in its canonical text form (ADR 0013).
pub fn new_id() -> String {
    todo!()
}

/// The directory for this session's sockets: under `XDG_RUNTIME_DIR` when
/// it is set (private to the user, by the XDG spec), otherwise in the
/// system's temporary directory.
pub fn runtime_dir(xdg_runtime_dir: Option<OsString>, temp_dir: &Path, id: &str) -> PathBuf {
    todo!()
}

/// Create `dir`, readable by its owner only, and its parent if missing.
/// Fails if `dir` exists: a session never shares one.
pub fn create_private(dir: &Path) -> io::Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_uuid_v7_text() {
        let id = new_id();
        let parsed = uuid::Uuid::parse_str(&id).unwrap();
        assert_eq!(parsed.get_version_num(), 7);
        assert_eq!(parsed.to_string(), id, "canonical form");
        assert_ne!(new_id(), id, "one per session");
    }

    #[test]
    fn the_runtime_dir_prefers_xdg() {
        assert_eq!(
            runtime_dir(Some("/run/user/1000".into()), Path::new("/tmp"), "abc"),
            PathBuf::from("/run/user/1000/lem-ratatui/abc")
        );
    }

    #[test]
    fn without_xdg_it_is_in_the_temporary_directory() {
        let expected = PathBuf::from("/tmp/lem-ratatui-abc");
        assert_eq!(runtime_dir(None, Path::new("/tmp"), "abc"), expected);
        assert_eq!(runtime_dir(Some("".into()), Path::new("/tmp"), "abc"), expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_directory_is_owner_only_and_new() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("lem-ratatui").join("abc");
        create_private(&dir).unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert!(create_private(&dir).is_err(), "never reused");
    }
}
```

Add `mod session;` to `src/main.rs`.

Run: `(cd rust && cargo test -p lem-ratatui-launcher)`
Expected: 4 tests FAIL with `not yet implemented`.

- [ ] **Step 3: Implement**

```rust
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub fn runtime_dir(xdg_runtime_dir: Option<OsString>, temp_dir: &Path, id: &str) -> PathBuf {
    match xdg_runtime_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("lem-ratatui").join(id),
        None => temp_dir.join(format!("lem-ratatui-{id}")),
    }
}

pub fn create_private(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}
```

Run: `(cd rust && cargo test -p lem-ratatui-launcher)`
Expected: PASS. (`runtime_dir` and `create_private` are unused until Task 6; if the dead-code lint fires, add `#[allow(dead_code)]` to those two with a comment `// Used from Task 6 on.` and remove it in Task 6.)

- [ ] **Step 4: The launcher passes the id to both halves**

In `src/main.rs`'s `run`, before the pipes:

```rust
    let session = session::new_id();
```

and on both commands, before `.stdin_file(...)`:

```rust
    let lem = duct::cmd(&programs.lisp, args)
        .env("LEM_SESSION_ID", &session)
        .stdin_file(lem_stdin)
```

```rust
    let display = duct::cmd(&programs.terminal, None::<OsString>)
        .env("LEM_SESSION_ID", &session)
        .stdin_file(from_lem)
```

- [ ] **Step 5: The display uses it (test first)**

In `rust/crates/lem-ratatui/src/main.rs`, add at the end:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launchers_session_id_is_used() {
        assert_eq!(session_id(Some("from-the-launcher".into())), "from-the-launcher");
    }

    #[test]
    fn without_one_the_display_makes_a_uuid_v7() {
        for missing in [None, Some(String::new())] {
            let id = session_id(missing);
            assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 7);
        }
    }
}
```

Run: `(cd rust && cargo test -p lem-ratatui session_id)` → FAIL: `session_id` not found.

Add, near the other free functions in `main.rs`:

```rust
/// This session's id: the launcher's, from `LEM_SESSION_ID` (ADR 0019),
/// or a new UUIDv7 when the display runs without one, as by hand.
fn session_id(from_launcher: Option<String>) -> String {
    from_launcher
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string())
}
```

and replace the two lines that make and send the session in `Hello`:

```rust
    let session = session_id(std::env::var("LEM_SESSION_ID").ok());
    writer.send(to_editor::Message::Hello(v1::Hello {
        protocol_version: v1::PROTOCOL_VERSION,
        session_id: session,
```

(was `let session = uuid::Uuid::now_v7();` and `session_id: session.to_string(),`).

- [ ] **Step 6: Run**

Run: `(cd rust && cargo test && cargo clippy --all-targets)`
Expected: all pass, no new warnings. Then the full build and `python3 scripts/acceptance.py`: 13/13.

- [ ] **Step 7: Commit**

```bash
git add frontends/ratatui/rust
git commit -m "feat(ratatui): the launcher makes the session id; the display uses it"
```

---

### Task 6: The launcher starts the agent

**Files:**
- Create: `rust/crates/lem-ratatui-launcher/src/agent.rs`
- Modify: `rust/crates/lem-ratatui-launcher/src/main.rs`, `src/bundle.rs`, `build.rs`, `frontends/ratatui/Makefile` (`dist`)

**Interfaces:**
- Consumes: Task 5's `session::{new_id, runtime_dir, create_private}`; Task 4's binary contract (`--socket PATH`, stdin lifeline, exit codes).
- Produces: Lem is started with `LEM_OS_AGENT_SOCKET` set when an agent started; the agent's stderr goes to `/tmp/lem-ratatui-os-agent.log`; `LEM_RATATUI_OS_AGENT` picks the agent binary; warnings printed after the session as `lem-ratatui: …`, containing "OS agent".

- [ ] **Step 1: Write `agent.rs` with failing tests**

```rust
//! The OS agent while a session runs (ADR 0019): told where to listen,
//! and kept alive by a pipe on its stdin that only this process holds.
//! Closing that pipe is how the agent learns the session is over; if
//! this process dies, the kernel closes it just the same.

use std::ffi::OsString;
use std::fs::File;
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::{Context, Result};

pub struct Agent {
    handle: duct::Handle,
    lifeline: Option<os_pipe::PipeWriter>,
}

impl Agent {
    /// Start `program --socket <socket>`, its stderr to `log`.
    pub fn start(program: &Path, socket: &Path, log: &Path) -> Result<Self> {
        todo!()
    }

    /// Close the lifeline, give the agent `grace` to exit, then kill it.
    /// `None` when it had to be killed.
    pub fn stop(mut self, grace: Duration) -> Result<Option<ExitStatus>> {
        todo!()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    /// A script standing in for the agent: it ignores its arguments.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("agent.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn closing_the_lifeline_ends_an_agent_that_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "cat >/dev/null");
        let agent = Agent::start(&program, &dir.path().join("s"), &dir.path().join("log")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(agent.handle.try_wait().unwrap().is_none(), "running while the pipe is open");
        let status = agent.stop(Duration::from_secs(5)).unwrap();
        assert!(status.expect("exited by itself").success());
    }

    #[test]
    fn an_agent_that_ignores_it_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "exec sleep 30");
        let agent = Agent::start(&program, &dir.path().join("s"), &dir.path().join("log")).unwrap();
        assert!(agent.stop(Duration::from_millis(100)).unwrap().is_none());
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-agent");
        assert!(Agent::start(&missing, &dir.path().join("s"), &dir.path().join("log")).is_err());
    }

    #[test]
    fn the_agent_is_told_its_socket() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let program = script(dir.path(), &format!("echo \"$@\" > {}", out.display()));
        let socket = dir.path().join("os-agent.sock");
        let agent = Agent::start(&program, &socket, &dir.path().join("log")).unwrap();
        agent.stop(Duration::from_secs(5)).unwrap();
        assert_eq!(
            std::fs::read_to_string(out).unwrap().trim(),
            format!("--socket {}", socket.display())
        );
    }
}
```

Add `mod agent;` to `main.rs`.

Run: `(cd rust && cargo test -p lem-ratatui-launcher agent)`
Expected: FAIL with `not yet implemented`.

- [ ] **Step 2: Implement**

```rust
    pub fn start(program: &Path, socket: &Path, log: &Path) -> Result<Self> {
        let (stdin, lifeline) = os_pipe::pipe().context("creating a pipe")?;
        let stderr = File::create(log).with_context(|| format!("creating {}", log.display()))?;
        let args = [OsString::from("--socket"), socket.as_os_str().to_owned()];
        let handle = duct::cmd(program, args)
            .stdin_file(stdin)
            .stdout_null()
            .stderr_file(stderr)
            .unchecked()
            .start()
            .with_context(|| format!("starting {}", program.display()))?;
        Ok(Self {
            handle,
            lifeline: Some(lifeline),
        })
    }

    pub fn stop(mut self, grace: Duration) -> Result<Option<ExitStatus>> {
        drop(self.lifeline.take());
        match self.handle.wait_timeout(grace)? {
            Some(output) => Ok(Some(output.status)),
            None => {
                self.handle.kill()?;
                let _ = self.handle.wait();
                Ok(None)
            }
        }
    }
```

Run: `(cd rust && cargo test -p lem-ratatui-launcher)`
Expected: PASS.

- [ ] **Step 3: Find the agent**

In `main.rs`, the agent log next to `LOG`:

```rust
/// Where the OS agent's stderr goes.
const AGENT_LOG: &str = "/tmp/lem-ratatui-os-agent.log";
```

`Programs` gains a field:

```rust
    /// The OS agent, when there is one. Optional: without it the session
    /// runs as before, and the clipboard is the display's.
    agent: Option<PathBuf>,
```

and `locate` finds it like the display, except that a development build without one beside the launcher is not an error:

```rust
        let agent = match env_path("LEM_RATATUI_OS_AGENT") {
            Some(path) => Some(path),
            #[cfg(feature = "bundle")]
            None => Some(bundle::os_agent()?),
            #[cfg(not(feature = "bundle"))]
            None => Some(
                std::env::current_exe()
                    .context("locating this executable")?
                    .with_file_name(format!("lem-os-agent{}", std::env::consts::EXE_SUFFIX)),
            )
            .filter(|path| path.is_file()),
        };
        Ok(Self { lisp, terminal, agent })
```

- [ ] **Step 4: Start it, tell Lem, stop it, report**

`Ended` gains:

```rust
    /// What went wrong without ending the session, reported after it.
    warnings: Vec<String>,
```

Add these two functions and the struct above `run`:

```rust
/// The OS agent while the session runs, and where its socket lives.
struct RunningAgent {
    agent: agent::Agent,
    dir: PathBuf,
    socket: PathBuf,
}

/// Start the OS agent in a new directory private to this session. Any
/// failure is a warning: Lem runs without an agent.
fn start_agent(program: &Path, session: &str, warnings: &mut Vec<String>) -> Option<RunningAgent> {
    let dir = session::runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"), &std::env::temp_dir(), session);
    if let Err(error) = session::create_private(&dir) {
        warnings.push(format!("no OS agent: creating {}: {error}", dir.display()));
        return None;
    }
    let socket = dir.join("os-agent.sock");
    match agent::Agent::start(program, &socket, Path::new(AGENT_LOG)) {
        Ok(agent) => Some(RunningAgent { agent, dir, socket }),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&dir);
            warnings.push(format!("no OS agent: {error:#}"));
            None
        }
    }
}

/// Stop the agent and remove its directory, noting anything abnormal.
fn stop_agent(running: RunningAgent, warnings: &mut Vec<String>) {
    match running.agent.stop(GRACE) {
        Ok(Some(status)) if !status.success() => {
            warnings.push(format!("the OS agent exited with {status}; see {AGENT_LOG}"));
        }
        Ok(Some(_)) => {}
        Ok(None) => warnings.push(format!("the OS agent did not exit and was killed; see {AGENT_LOG}")),
        Err(error) => warnings.push(format!("stopping the OS agent: {error:#}")),
    }
    let _ = std::fs::remove_dir_all(&running.dir);
}
```

In `run`: start the agent first, give Lem the socket, and stop the agent after both halves are gone. The function becomes:

```rust
fn run(programs: &Programs, args: &[OsString], log: &Path) -> Result<Ended> {
    let session = session::new_id();
    let mut warnings = Vec::new();
    let agent = programs
        .agent
        .as_deref()
        .and_then(|program| start_agent(program, &session, &mut warnings));

    // One pipe each way. Lem is started with its ends and the display with
    // the others; the expressions holding them are dropped as soon as each
    // has started, so this process keeps no end open, and either half
    // exiting is seen by the other as EOF.
    let (from_lem, lem_stdout) = os_pipe::pipe().context("creating a pipe")?;
    let (lem_stdin, to_lem) = os_pipe::pipe().context("creating a pipe")?;
    let stderr = File::create(log).with_context(|| format!("creating {}", log.display()))?;

    let mut lem = duct::cmd(&programs.lisp, args).env("LEM_SESSION_ID", &session);
    if let Some(running) = &agent {
        lem = lem.env("LEM_OS_AGENT_SOCKET", &running.socket);
    }
    let lem = lem
        .stdin_file(lem_stdin)
        .stdout_file(lem_stdout)
        .stderr_file(stderr)
        .unchecked()
        .start();
    let lem = match lem {
        Ok(lem) => lem,
        Err(error) => {
            if let Some(running) = agent {
                stop_agent(running, &mut warnings);
            }
            return Err(error).with_context(|| format!("starting {}", programs.lisp.display()));
        }
    };
    let display = duct::cmd(&programs.terminal, None::<OsString>)
        .env("LEM_SESSION_ID", &session)
        .stdin_file(from_lem)
        .stdout_file(to_lem)
        .unchecked()
        .start();
    let display = match display {
        Ok(display) => display,
        Err(error) => {
            let _ = lem.kill();
            if let Some(running) = agent {
                stop_agent(running, &mut warnings);
            }
            return Err(error).with_context(|| format!("starting {}", programs.terminal.display()));
        }
    };

    let display = display.wait()?.status;
    let lem = match lem.wait_timeout(GRACE)? {
        Some(output) => Some(output.status),
        None => {
            lem.kill()?;
            lem.wait()?;
            None
        }
    };
    if let Some(running) = agent {
        stop_agent(running, &mut warnings);
    }
    Ok(Ended { display, lem, warnings })
}
```

`report` prints the warnings first:

```rust
fn report(ended: &Ended, log: &Path) -> ExitCode {
    for warning in &ended.warnings {
        eprintln!("lem-ratatui: {warning}");
    }
    if !ended.display.success() {
```

(the rest unchanged). Update the module doc's diagram at the top of `main.rs` to show the agent:

```rust
//! Starts Lem, its terminal display and the OS agent, and connects them.
//!
//! The three are siblings under this process, not parent and child:
//!
//! ```text
//!                      lem-ratatui-launcher
//!                     /         |          \
//!   lem-ratatui-lisp  ─ stdio ─►  lem-ratatui     lem-os-agent
//!   (lem.display.v1)  ◄─ stdio ─  (owns /dev/tty) (lem.os.v1, on a
//!         └────────────── local socket ─────────► socket it was named)
//! ```
//!
//! None of them knows how the others were found or started. Everything
//! about the operating system — where the programs live, unpacking the
//! bundled ones, where logs and sockets go, what an exit means — belongs
//! here. The agent is optional: a session without one runs as before.
```

- [ ] **Step 5: The bundle carries the agent**

`build.rs`: three payloads.

```rust
    const PAYLOADS: [(&str, &str, &str); 3] = [
        (
            "LEM_RATATUI_LISP",
            "../../../dist/lem-ratatui-lisp",
            "lem-ratatui-lisp.zst",
        ),
        (
            "LEM_RATATUI_TERMINAL",
            "../../target/dist/lem-ratatui",
            "lem-ratatui.zst",
        ),
        (
            "LEM_RATATUI_OS_AGENT",
            "../../target/dist/lem-os-agent",
            "lem-os-agent.zst",
        ),
    ];
```

`src/bundle.rs`: after `TERMINAL`,

```rust
static OS_AGENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lem-os-agent.zst"));
```

and after `terminal()`,

```rust
/// Path of the unpacked OS agent.
pub fn os_agent() -> Result<PathBuf> {
    install(&format!("lem-os-agent{EXE_SUFFIX}"), OS_AGENT)
}
```

Update the module doc's first line to "The Lisp image, display binary and OS agent embedded in this one (`bundle` feature)."

`Makefile`, the `$(DIST_BIN)` recipe:

```make
$(DIST_BIN): $(LISP_BIN) FORCE
	cd $(HERE)/rust && $(CARGO) build --profile dist -p lem-ratatui -p lem-os-agent
	cd $(HERE)/rust && LEM_RATATUI_LISP=$(LISP_BIN) LEM_RATATUI_TERMINAL=$(HERE)/rust/target/dist/lem-ratatui \
		LEM_RATATUI_OS_AGENT=$(HERE)/rust/target/dist/lem-os-agent \
		$(CARGO) build --profile dist -p lem-ratatui-launcher --features bundle
	cp $(HERE)/rust/target/dist/lem-ratatui-launcher $@
```

and its comment: "The launcher, with the Lisp image, the display binary and the OS agent embedded (zstd) … The display binary and the agent are built first because the launcher's build script reads them."

- [ ] **Step 6: Run**

Run: `(cd rust && cargo test && cargo clippy --all-targets && cargo fmt --check)`
Expected: all pass; the only fmt diff is the pre-existing one in `lem-protocols/tests/display_v1_fixture.rs`.

Run the full build, then `python3 scripts/acceptance.py`.
Expected: 13/13. Lem does not connect to the agent yet (Task 7–8), so behaviour is unchanged, but `/tmp/lem-ratatui-os-agent.log` now exists after a run, and `ls $XDG_RUNTIME_DIR/lem-ratatui/` is empty afterwards.

Run: `make dist` (takes a while), then `ls -la dist/lem-ratatui`.
Expected: builds; the binary exists.

- [ ] **Step 7: Commit**

```bash
git add frontends/ratatui/rust frontends/ratatui/Makefile
git commit -m "feat(ratatui): the launcher starts the OS agent and tells Lem where it is"
```

---

### Task 7: `lem-os-agent` in Lisp

**Files:**
- Create: `os-agent/lem-os-agent.asd`, `os-agent/codec.lisp`, `os-agent/main.lisp`, `os-agent/tests/main.lisp`
- Modify: `Makefile` (`LISP_SOURCES`, `test-lisp`)

**Interfaces:**
- Consumes: Task 1's `lem-ipc-channels`; Task 3's `cl-protobufs.lem.os.v1`.
- Produces (package `lem-os-agent`, all exported):
  - `agent` (class), `(connect path session-id &key timeout)` → agent or `NIL`, `(disconnect &optional agent)`.
  - `(start &key path session-id)` → thread or `NIL` (no socket given): connects in the background and sets the current agent.
  - `(current-agent)` → agent or `NIL`; `(connected-p &optional agent)`; `(capable-p capability &optional agent)` with `capability` a keyword (`:clipboard`).
  - `(clipboard-text &optional agent)` → string or `NIL`; `(set-clipboard-text text &optional agent)` → `T` when sent, `NIL` when the agent has no clipboard.
  - `*connect-timeout*` (1), `*request-timeout*` (0.1).

- [ ] **Step 1: The system**

`os-agent/lem-os-agent.asd`:

```lisp
(defsystem "lem-os-agent"
  :description "Lem's side of the link to the OS agent (ADR 0019): lem.os.v1
over a local socket, and the API Lem, extensions and init files use. Needs
`make toolchain', as lem-display-relay/protobuf does: loading compiles the
schema with protoc and cl-protobufs' plugin."
  :defsystem-depends-on ("cl-protobufs.asdf")
  :depends-on ("lem-ipc-channels" "cl-protobufs" "alexandria" "bordeaux-threads" "log4cl")
  :serial t
  :components ((:protobuf-source-file "os"
                :proto-pathname "../proto/lem/os/v1/os.proto")
               (:file "codec")
               (:file "main"))
  :in-order-to ((test-op (test-op "lem-os-agent/tests"))))

(defsystem "lem-os-agent/tests"
  :depends-on ("lem-os-agent" "rove")
  :pathname "tests/"
  :components ((:file "main"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
```

In `Makefile`, add `$(HERE)/os-agent` to `LISP_SOURCES`' `find`, and a third line to `test-lisp`:

```make
	cd $(ROOT) && $(WITH_TOOLCHAIN) .qlot/bin/rove frontends/ratatui/os-agent/lem-os-agent.asd
```

- [ ] **Step 2: Write the failing tests**

`os-agent/tests/main.lisp`:

```lisp
(defpackage :lem-os-agent/tests
  (:use :cl :rove)
  (:local-nicknames (:agent :lem-os-agent)
                    (:ipc :lem-ipc-channels)
                    (:pb :cl-protobufs.lem.os.v1)
                    (:proto :cl-protobufs)))
(in-package :lem-os-agent/tests)

(defun temporary-socket-path ()
  (format nil "~Alem-os-agent-test-~36R.sock"
          (namestring (uiop:temporary-directory))
          (random (expt 36 8) (make-random-state t))))

(defun to-editor (seq &rest message)
  (apply #'pb:make-to-editor :seq seq :time-us 0 message))

(defun welcome (reply-to &key (clipboard t) (version 1))
  (to-editor 1 :welcome (pb:make-welcome :protocol-version version
                                         :session-id "test"
                                         :reply-to reply-to
                                         :capabilities (pb:make-capabilities
                                                        :clipboard clipboard))))

(defun call-with-fake-agent (respond function)
  "Call FUNCTION with the path of a socket where a fake agent listens.
RESPOND is called with each `ToAgent' it receives and returns the
`ToEditor' messages to answer with, or :hang-up to close the connection.
The fake serves one connection."
  (let* ((path (temporary-socket-path))
         (listener (make-instance 'sb-bsd-sockets:local-socket :type :stream)))
    (sb-bsd-sockets:socket-bind listener path)
    (sb-bsd-sockets:socket-listen listener 1)
    (bt2:make-thread
     (lambda ()
       (let* ((peer (sb-bsd-sockets:socket-accept listener))
              (stream (sb-bsd-sockets:socket-make-stream peer :input t :output t
                                                              :element-type '(unsigned-byte 8)
                                                              :buffering :full)))
         (unwind-protect
              (loop :for body := (ipc:read-delimited stream)
                    :while body
                    :do (let ((answer (funcall respond
                                               (proto:deserialize-from-bytes 'pb:to-agent body))))
                          (when (eq answer :hang-up)
                            (return))
                          (dolist (message answer)
                            (ipc:write-delimited stream (proto:serialize-to-bytes message)))))
           (sb-bsd-sockets:socket-close peer))))
     :name "fake lem-os-agent")
    ;; The fake's thread is not joined: one whose client never connected
    ;; stays in accept, harmlessly, until the test process ends.
    (unwind-protect (funcall function path)
      (sb-bsd-sockets:socket-close listener)
      (ignore-errors (delete-file path)))))

(defmacro with-fake-agent ((path respond) &body body)
  `(call-with-fake-agent ,respond (lambda (,path) ,@body)))

(defun clipboard-agent (&key (text "from the agent") (clipboard t) set)
  "A RESPOND for a fake agent whose clipboard holds TEXT (NIL for nothing
readable). SET, when given, is called with each text Lem copies."
  (lambda (message)
    (let ((seq (pb:to-agent.seq message)))
      (case (pb:to-agent.message-case message)
        (pb:hello (list (welcome seq :clipboard clipboard)))
        (pb:clipboard-request
         (list (to-editor 2 :clipboard-reply
                          (apply #'pb:make-clipboard-reply :reply-to seq
                                 (and text (list :text text))))))
        (pb:set-clipboard
         (when set
           (funcall set (pb:set-clipboard.text (pb:to-agent.set-clipboard message))))
         '())))))

(defun welcome-only (message)
  "A RESPOND that welcomes, then never answers."
  (when (eq 'pb:hello (pb:to-agent.message-case message))
    (list (welcome (pb:to-agent.seq message)))))

;;; Connecting

(deftest connecting-learns-what-the-agent-can-do
  (with-fake-agent (path (clipboard-agent))
    (let ((agent (agent:connect path "test")))
      (ok agent)
      (ok (agent:connected-p agent))
      (ok (agent:capable-p :clipboard agent))
      (ok (not (agent:capable-p :tray agent)) "nothing it did not announce")
      (agent:disconnect agent))))

(deftest no-agent-is-nil
  (ok (null (agent:connect (temporary-socket-path) "test" :timeout 0.1))))

(deftest an-agent-that-never-welcomes-is-no-agent
  (with-fake-agent (path (lambda (message) (declare (ignore message)) '()))
    (ok (null (agent:connect path "test" :timeout 0.2)))))

(deftest a-welcome-without-a-protocol-version-is-no-agent
  (with-fake-agent (path (lambda (message)
                           (list (welcome (pb:to-agent.seq message) :version 0))))
    (ok (null (agent:connect path "test")))))

(deftest without-a-socket-start-does-nothing
  (ok (null (agent:start :path nil)))
  (ok (null (agent:start :path ""))))

;;; The clipboard

(deftest an-agent-without-a-clipboard-is-not-asked
  (with-fake-agent (path (clipboard-agent :clipboard nil))
    (let ((agent (agent:connect path "test")))
      (ok (not (agent:capable-p :clipboard agent)))
      (ok (null (agent:clipboard-text agent)))
      (ok (null (agent:set-clipboard-text "x" agent)))
      (agent:disconnect agent))))

(deftest the-clipboard-comes-from-the-agent
  (with-fake-agent (path (clipboard-agent :text "from the agent"))
    (let ((agent (agent:connect path "test")))
      (ok (equal "from the agent" (agent:clipboard-text agent)))
      (agent:disconnect agent))))

(deftest nothing-on-the-clipboard-is-nil
  (with-fake-agent (path (clipboard-agent :text nil))
    (let ((agent (agent:connect path "test")))
      (ok (null (agent:clipboard-text agent)))
      (agent:disconnect agent))))

(deftest copying-puts-text-on-the-agents-clipboard
  (let ((copied nil)
        (arrived (bt2:make-semaphore)))
    (with-fake-agent (path (clipboard-agent :set (lambda (text)
                                                   (setf copied text)
                                                   (bt2:signal-semaphore arrived))))
      (let ((agent (agent:connect path "test")))
        (ok (agent:set-clipboard-text "copied" agent))
        (ok (bt2:wait-on-semaphore arrived :timeout 2))
        (ok (equal "copied" copied))
        (agent:disconnect agent)))))

(deftest a-silent-agent-costs-only-the-request-timeout
  (with-fake-agent (path #'welcome-only)
    (let ((agent (agent:connect path "test"))
          (start (get-internal-real-time)))
      (ok (null (agent:clipboard-text agent)))
      (ok (< (/ (- (get-internal-real-time) start) internal-time-units-per-second) 1)
          "bounded by *request-timeout*")
      (agent:disconnect agent))))

(deftest when-the-agent-hangs-up-it-can-do-nothing
  (with-fake-agent (path (lambda (message)
                           (if (eq 'pb:hello (pb:to-agent.message-case message))
                               (list (welcome (pb:to-agent.seq message)))
                               :hang-up)))
    (let ((agent (agent:connect path "test")))
      (ok (agent:capable-p :clipboard agent))
      (ok (null (agent:clipboard-text agent)) "the request it hung up on")
      (sleep 0.2)
      (ok (not (agent:connected-p agent)))
      (ok (not (agent:capable-p :clipboard agent)))
      (ok (null (agent:set-clipboard-text "x" agent)) "and nothing is sent"))))
```

Run: `make test-lisp`
Expected: `lem-os-agent/tests` FAIL to load: `codec.lisp` and `main.lisp` do not exist.

- [ ] **Step 3: Write `codec.lisp`**

```lisp
(defpackage :lem-os-agent/codec
  (:use :cl)
  (:local-nicknames (:pb :cl-protobufs.lem.os.v1)
                    (:proto :cl-protobufs))
  (:import-from :lem-ipc-channels :now-microseconds)
  (:export :+protocol-version+
           :hello
           :clipboard-request
           :set-clipboard
           :decode))
(in-package :lem-os-agent/codec)

(defconstant +protocol-version+ 1
  "The revision of lem.os.v1 this codec speaks (os.proto, `Hello').")

(defun envelope (seq &rest message)
  (proto:serialize-to-bytes
   (apply #'pb:make-to-agent :seq seq :time-us (now-microseconds) message)))

(defun hello (seq session-id)
  "A serialised `Hello' numbered SEQ, introducing this editor as SESSION-ID."
  (envelope seq :hello (pb:make-hello :protocol-version +protocol-version+
                                      :session-id session-id)))

(defun clipboard-request (seq)
  "A serialised `ClipboardRequest' numbered SEQ."
  (envelope seq :clipboard-request (pb:make-clipboard-request)))

(defun set-clipboard (seq text)
  "A serialised `SetClipboard' numbered SEQ, putting TEXT on the clipboard."
  (envelope seq :set-clipboard (pb:make-set-clipboard :text text)))

(defun capabilities-from (welcome)
  "What WELCOME says the agent can do, as a list of keywords."
  (when (pb:welcome.has-capabilities welcome)
    (let ((capabilities (pb:welcome.capabilities welcome)))
      (append (and (pb:capabilities.clipboard capabilities) (list :clipboard))))))

(defun decode (body)
  "What BODY, one serialised `ToEditor' from the agent, says:

  (:welcome REPLY-TO PROTOCOL-VERSION CAPABILITIES)
  (:clipboard-reply REPLY-TO TEXT)   ; TEXT is NIL when nothing is readable
  (:ignored)                         ; a message this revision does not know"
  (let ((message (proto:deserialize-from-bytes 'pb:to-editor body)))
    (case (pb:to-editor.message-case message)
      (pb:welcome
       (let ((welcome (pb:to-editor.welcome message)))
         (list :welcome
               (pb:welcome.reply-to welcome)
               (pb:welcome.protocol-version welcome)
               (capabilities-from welcome))))
      (pb:clipboard-reply
       (let ((reply (pb:to-editor.clipboard-reply message)))
         (list :clipboard-reply
               (pb:clipboard-reply.reply-to reply)
               (and (pb:clipboard-reply.has-text reply)
                    (pb:clipboard-reply.text reply)))))
      (t (list :ignored)))))
```

- [ ] **Step 4: Write `main.lisp`**

```lisp
(defpackage :lem-os-agent
  (:use :cl)
  (:local-nicknames (:ipc :lem-ipc-channels)
                    (:codec :lem-os-agent/codec))
  (:export :*connect-timeout*
           :*request-timeout*
           :agent
           :connect
           :disconnect
           :start
           :current-agent
           :connected-p
           :capable-p
           :clipboard-text
           :set-clipboard-text))
(in-package :lem-os-agent)

(defvar *agent* nil
  "The agent this editor is connected to, set by `start' once the agent
answers; NIL before that, and when there is none. Read it with
`current-agent'.")

(defparameter *connect-timeout* 1
  "Seconds `connect' tries the agent's socket, and then waits for its
`Welcome'. The launcher starts the agent before Lem, so it is normally
listening long before Lem asks.")

(defparameter *request-timeout* 0.1
  "Seconds a request waits for the agent's answer, as long as Lem waits
for the display's clipboard: Lem asks on every yank.")

(defclass agent ()
  ((channel :initarg :channel :reader agent-channel)
   (replies :initform (ipc:make-replies) :reader agent-replies)
   (session-id :initarg :session-id :reader agent-session-id)
   (capabilities
    :initform '()
    :accessor agent-capabilities
    :documentation "Keywords from the agent's `Welcome': what it can do.")
   (connected
    :initform t
    :accessor agent-connected
    :documentation "NIL once the agent has hung up, from the reader thread."))
  (:documentation "A connection to the OS agent (ADR 0019): a channel, the
replies awaited on it, and what the agent said it can do."))

(defun handle (agent body)
  "Hand one message from the agent to whoever awaits it. Runs on the
reader thread."
  (let ((message (codec:decode body)))
    (ecase (first message)
      (:welcome
       (destructuring-bind (reply-to version capabilities) (rest message)
         (ipc:deliver-reply (agent-replies agent) reply-to (list version capabilities))))
      (:clipboard-reply
       (destructuring-bind (reply-to text) (rest message)
         (ipc:deliver-reply (agent-replies agent) reply-to text)))
      (:ignored nil))))

(defun connect (path session-id &key (timeout *connect-timeout*))
  "Connect to the agent listening at PATH, introducing this editor as
SESSION-ID. Return an `agent', or NIL when no agent is there or it does
not welcome this editor within TIMEOUT seconds."
  (alexandria:when-let ((channel (ipc:connect-local path :timeout timeout)))
    (let ((agent (make-instance 'agent :channel channel :session-id session-id)))
      (ipc:start-reader channel
                        (lambda (body) (handle agent body))
                        :name "lem-os-agent reader"
                        :on-close (lambda () (setf (agent-connected agent) nil)))
      (multiple-value-bind (welcome arrived)
          (ipc:await-reply (agent-replies agent)
                           (ipc:channel-send channel (lambda (seq) (codec:hello seq session-id)))
                           timeout)
        (cond ((and arrived (plusp (first welcome)))
               (setf (agent-capabilities agent) (second welcome))
               agent)
              (t
               (log:warn "lem-os-agent: no Welcome from ~A" path)
               (ipc:close-channel channel)
               nil))))))

(defun current-agent ()
  "The agent this editor is connected to, or NIL."
  *agent*)

(defun disconnect (&optional (agent (current-agent)))
  "Close the connection to AGENT."
  (when agent
    (ipc:close-channel (agent-channel agent))
    (setf (agent-connected agent) nil)))

(defun start (&key (path (uiop:getenv "LEM_OS_AGENT_SOCKET"))
                   (session-id (or (uiop:getenv "LEM_SESSION_ID") "")))
  "Connect to the agent the launcher started, at PATH, on a new thread, and
make it the `current-agent' once it answers. Returns the thread, or NIL
when there is no PATH. Until the agent answers, and if it never does, Lem
runs as if there were none."
  (when (and path (plusp (length path)))
    (bt2:make-thread (lambda ()
                       ;; An error escaping this thread would end the
                       ;; process: the debugger is disabled (transport.lisp).
                       (setf *agent* (handler-case (connect path session-id)
                                       (error (condition)
                                         (log:warn "lem-os-agent: connecting: ~A" condition)
                                         nil))))
                     :name "lem-os-agent connect")))

(defun connected-p (&optional (agent (current-agent)))
  "True when AGENT is there and has not hung up."
  (and agent (agent-connected agent) t))

(defun capable-p (capability &optional (agent (current-agent)))
  "True when AGENT is connected and said it can do CAPABILITY, a keyword
such as :clipboard."
  (and (connected-p agent)
       (member capability (agent-capabilities agent))
       t))

(defun clipboard-text (&optional (agent (current-agent)))
  "The text on the desktop's clipboard, through AGENT; NIL when it has no
clipboard, nothing readable is on it, or it does not answer within
`*request-timeout*'."
  (when (capable-p :clipboard agent)
    (handler-case
        (values (ipc:await-reply (agent-replies agent)
                                 (ipc:channel-send (agent-channel agent)
                                                   #'codec:clipboard-request)
                                 *request-timeout*))
      (error (condition)
        (log:warn "lem-os-agent: asking for the clipboard: ~A" condition)
        nil))))

(defun set-clipboard-text (text &optional (agent (current-agent)))
  "Put TEXT on the desktop's clipboard, through AGENT. T when sent; NIL when
AGENT has no clipboard or the connection failed."
  (when (capable-p :clipboard agent)
    (handler-case
        (progn
          (ipc:channel-send (agent-channel agent)
                            (lambda (seq) (codec:set-clipboard seq text)))
          t)
      (error (condition)
        (log:warn "lem-os-agent: setting the clipboard: ~A" condition)
        nil))))
```

- [ ] **Step 5: Run the tests**

Run: `make test-lisp`
Expected: all three suites pass. If `when-the-agent-hangs-up-it-can-do-nothing` fails on `connected-p`, the reader's `on-close` did not run: check the fake really closes (`:hang-up`) and that `start-reader` calls `on-close` in its `unwind-protect`.

- [ ] **Step 6: Commit**

```bash
git add frontends/ratatui/os-agent frontends/ratatui/Makefile
git commit -m "feat(ratatui): lem-os-agent in Lisp, the agent link and its API"
```

---

### Task 8: Lem uses the agent's clipboard

**Files:**
- Create: `lisp/clipboard.lisp`
- Modify: `lem-ratatui.asd`, `lisp/main.lisp`, `scripts/acceptance.py`, `README.md`

**Interfaces:**
- Consumes: Task 7's `lem-os-agent:{start, capable-p, clipboard-text, set-clipboard-text}`; Task 6's launcher behaviour (`LEM_OS_AGENT_SOCKET`, warnings); Task 4's `LEM_OS_AGENT_CLIPBOARD_FILE`.
- Produces: the finished feature.

- [ ] **Step 1: Write the failing acceptance checks**

In `scripts/acceptance.py`, add `import tempfile` to the imports, and before the summary at the end:

```python
# --- 14: the OS agent's clipboard (ADR 0019) ---------------------------
# A file stands in for the desktop's clipboard (LEM_OS_AGENT_CLIPBOARD_FILE),
# so this runs with no display server. Yanking inserts what the agent says
# is on the clipboard; copying puts text there. XDG_RUNTIME_DIR points at a
# scratch directory so that the session's socket directory can be seen to
# go away.
runtime = tempfile.mkdtemp(prefix="lem-accept-runtime-")
board = os.path.join(runtime, "clipboard.txt")
with open(board, "w") as f:
    f.write("from-the-agent")
path = "/tmp/lem-accept-agent.txt"
if os.path.exists(path): os.remove(path)
s = Session(env={"LEM_OS_AGENT_CLIPBOARD_FILE": board, "XDG_RUNTIME_DIR": runtime}); s.pump(8)
s.send([b"\x18", b"\x06"]); s.pump(1.5)                              # C-x C-f
s.send(path); s.send([b"\r"]); s.pump(2)
s.send([b"\x19"]); s.pump(1)                                         # C-y
s.send([b"\r"]); s.send("copied", delay=0.15); s.pump(0.6)
s.send([b"\x01", b"\x00", b"\x05", b"\x1bw"], delay=0.25); s.pump(1)  # C-a C-SPC C-e M-w
s.send([b"\x18", b"\x13"]); s.pump(2)                                # C-x C-s
s.send([b"\x18", b"\x03"]); s.wait(8); s.kill()                      # C-x C-c
content = open(path).read() if os.path.exists(path) else "<no file>"
copied = open(board).read()
sessions = os.path.join(runtime, "lem-ratatui")
left = os.listdir(sessions) if os.path.isdir(sessions) else []
check(14, "the OS agent's clipboard is Lem's, and its socket is cleaned up",
      content.startswith("from-the-agent\ncopied") and copied == "copied" and left == [],
      f"file={content!r}, clipboard={copied!r}, left behind={left}")

# --- 15: no OS agent ----------------------------------------------------
# The agent is optional: when it cannot start, the editor runs as before
# and the launcher says why once the terminal is back.
s = Session(env={"LEM_RATATUI_OS_AGENT": "/nonexistent/lem-os-agent"}); s.pump(8)
started = s.plain()
s.send([b"\x18", b"\x03"]); status = s.wait(8); s.kill()
told = "OS agent" in s.plain()
check(15, "without an OS agent the editor runs, and the launcher says why",
      "Welcome to Lem" in started and told and status == 0,
      f"started={'Welcome to Lem' in started}, warned={told}, status={status}")
```

Run the full build, then `python3 scripts/acceptance.py`.
Expected: 1–13 PASS; 14 FAIL (`file='\ncopied'`-like: the yank came from the display, which has no clipboard here, and the agent's file is untouched); 15 PASS (Task 6 already warns). If 15 fails on the status, print `status` and compare with check 7's handling: `wait` returns the raw `waitpid` status, 0 for a clean exit.

- [ ] **Step 2: Route the clipboard**

`lisp/clipboard.lisp`:

```lisp
(defpackage :lem-ratatui/clipboard
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui))
(in-package :lem-ratatui/clipboard)

;;; Whose clipboard Lem uses (ADR 0019): the desktop's, through the OS
;;; agent, when the agent has one; otherwise the display's, through the
;;; relay. The display's is where the terminal is, which over SSH is the
;;; only clipboard on the user's own machine.

(defmethod lem-if:clipboard-copy ((ratatui ratatui) text)
  (unless (lem-os-agent:set-clipboard-text text)
    (call-next-method)))

(defmethod lem-if:clipboard-paste ((ratatui ratatui))
  (if (lem-os-agent:capable-p :clipboard)
      (lem-os-agent:clipboard-text)
      (call-next-method)))
```

`lem-ratatui.asd`: add `"lem-os-agent"` after `"lem-display-relay/protobuf"` in `:depends-on`, and the component after `implementation`:

```lisp
  :components ((:file "implementation")
               (:file "clipboard")
               (:file "transport")
               (:file "links")
               (:file "main")))
```

- [ ] **Step 3: Connect when the editor starts**

In `lisp/main.lisp`, `lem-if:invoke` starts the connection before serving the display:

```lisp
(defmethod lem-if:invoke ((ratatui ratatui) function)
  "Run the editor, relaying it over the protocol streams in lem.display.v1,
with the OS agent connected in the background when the launcher started
one (ADR 0019). A crash report the editor leaves also reaches the
display, in `Exit'; here it goes to the log, since stdout is the wire."
  (lem-os-agent:start)
  (alexandria:when-let ((report (lem-display-relay/protobuf/serve:serve ratatui function
                                                                :input *protocol-input*
                                                                :output *protocol-output*)))
    (format *log-stream* "~&~A~%" report)
    (force-output *log-stream*)))
```

- [ ] **Step 4: Run everything**

Run the full build, then:

- `make test` → all Rust and Lisp suites pass.
- `python3 scripts/acceptance.py` → 15/15.
- `(cd rust && cargo build -p lem-ratatui) && python3 scripts/keyboard-acceptance.py` → 8/8.

- [ ] **Step 5: Document it**

In `README.md`:

1. **Shape.** Replace the opening of the section and the diagram with:

   ````markdown
   Three processes under a launcher, split by toolchain and by job:

   ```
   ipc-channels/  lem-ipc-channels: framing, numbering, replies and a
                  reader thread, for every link to Lem (ADR 0019)
   display-relay/ lem-display-relay: every lem-if:* method, building frames
                  from what Lem draws, and the codec that puts them on the wire
   os-agent/      lem-os-agent: Lem's side of the link to the OS agent, and
                  the API extensions use
   lisp/          the ratatui implementation class and the process entry point
   rust/          the launcher, which starts the others and connects them;
                  the display, which owns the terminal; and the OS agent,
                  which serves the desktop's clipboard
   docs/          the investigation, and the decisions it produced
   ```
   ````

   and under the existing process diagram add:

   ```markdown
   The launcher also starts `lem-os-agent`, the OS agent, when it finds
   one, with a socket in a directory private to the session; Lem connects
   to it there (`lem.os.v1`). It is optional: without it the clipboard is
   the display's. See [`docs/adr/0019`](docs/adr/0019-an-os-agent-and-a-link-per-peer.md).
   ```

2. **Layout.** Add, in order, beside the existing entries:

   ```
   ipc-channels/                lem-ipc-channels (ADR 0019)
     lem-ipc-channels.asd       lem-ipc-channels, its /tests
     framing.lisp, replies.lisp, channel.lisp
   os-agent/                    lem-os-agent, Lem's side of the agent link
     lem-os-agent.asd           lem-os-agent, its /tests
     codec.lisp, main.lisp
   proto/lem/os/v1/
     os.proto                   the agent link's schema (ADR 0019)
   ```

   under `lisp/`: `  clipboard.lisp             the agent's clipboard, else the display's`; and under `crates/`: `    lem-os-agent/            the OS agent: the desktop's clipboard over lem.os.v1`.

3. **Building.** After "`LEM_RATATUI_LISP` picks the Lisp image and `LEM_RATATUI_TERMINAL` the display binary", add: "and `LEM_RATATUI_OS_AGENT` the OS agent (one that does not exist means none: the launcher says so and carries on)". In the `make dist` paragraph, "the Lisp image and the display binary" becomes "the Lisp image, the display binary and the OS agent".

4. **Clipboard.** Replace the first paragraph with:

   ```markdown
   `M-w` and `C-y` use the desktop's clipboard through the OS agent when it
   has one, and otherwise the display's, both through `arboard`. Lem waits
   only 0.1s for a paste to be answered, so the read happens inline rather
   than on a thread. Over SSH the agent finds no display server and says
   so, and the display's clipboard is used.

   `LEM_OS_AGENT_CLIPBOARD_FILE=PATH` makes a file the agent's clipboard,
   for trying it without a desktop; acceptance check 14 does.
   ```

   and in the X11 caveat, "text copied out of Lem is gone once Lem exits" stays true: the agent exits with the session.

5. **Acceptance.** Wherever the README states the number of acceptance checks, make it 15.

- [ ] **Step 6: Commit**

```bash
git add frontends/ratatui/lisp frontends/ratatui/lem-ratatui.asd frontends/ratatui/scripts/acceptance.py frontends/ratatui/README.md
git commit -m "feat(ratatui): Lem's clipboard is the OS agent's when it has one"
```

---

## Self-review notes

- **Spec coverage (ADR 0019).** Separate packages per link: Task 3. `lem-protocols` holds both: Task 3. `lem-os-agent` crate, listening, no capabilities without a desktop: Task 4. Launcher protocol-free, finds the agent (`LEM_RATATUI_OS_AGENT`, bundle, beside itself), private dir, `--socket`, stdin lifeline, `LEM_OS_AGENT_SOCKET`, warning not failure, directory removed: Tasks 5–6. Session id UUIDv7 via `LEM_SESSION_ID`, display falls back: Task 5. `lem-ipc-channels` extracted from framing, serve and the reply queue: Tasks 1–2. `lem-display-relay` on it: Task 2. `lem-os-agent` Lisp: typed API, no generated classes exported, independent of the relay, connects with retry, no-op without an agent: Task 7. Clipboard arbitration in the `ratatui` class: Task 8. `make dist` embeds a third binary: Task 6.
- **Not in this plan, by the ADR's own account:** notifications, tray, clipboard history (later additions within v1), OSC 52 in the display, whether the display keeps `arboard`.
