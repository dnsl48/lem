(defpackage :lem-ratatui/transport
  (:use :cl)
  (:export :*protocol-input*
           :*protocol-output*
           :*log-stream*
           :start-debug-watchdog
           :*backtrace-delay*
           :call-with-protocol-streams))
(in-package :lem-ratatui/transport)

(defvar *protocol-input* nil
  "The real stdin, held apart from `*standard-input*'.
Bound by `call-with-protocol-streams'.")

(defvar *protocol-output* nil
  "The real stdout, held apart from `*standard-output*'.

The wire and any stray editor output would otherwise share one stream,
and they cannot: muffling `*standard-output*' to protect the wire would
also silence the wire. Keeping the protocol on its own stream lets
`*standard-output*' be redirected to a sink without the relay noticing.")

(defvar *backtrace-delay* nil
  "Seconds to wait before dumping every thread's backtrace, or NIL.
Set from LEM_RATATUI_BACKTRACE at startup.")

(defvar *log-stream* nil
  "The process's real stderr, captured before anything rebinds it.

`*error-output*' cannot be used directly: most of our code runs on the
editor thread, which `run-editor-thread' wraps in `with-editor-stream',
and that rebinds the standard streams. Diagnostics written there vanish
into the editor rather than reaching the log the launcher captures.")

(defun start-debug-watchdog (&key (delay 6))
  "After DELAY seconds, dump every thread's backtrace to stderr.

A protocol child has no REPL and no debugger, so when the editor thread
fails to make progress there is otherwise no way to see where it is
parked. Gated on LEM_RATATUI_BACKTRACE rather than LEM_RATATUI_DEBUG
because it interrupts every running thread, which is too invasive to do
merely because logging is on."
  (let ((log *error-output*))
    (sb-thread:make-thread
     (lambda ()
       (sleep delay)
       (dolist (thread (sb-thread:list-all-threads))
         (let ((name (sb-thread:thread-name thread)))
           (format log "~&[lem-ratatui] === thread ~A ===~%" name)
           (force-output log)
           (unless (eq thread sb-thread:*current-thread*)
             (ignore-errors
              (sb-thread:interrupt-thread
               thread
               (lambda ()
                 (sb-debug:print-backtrace :stream log :count 25)
                 (force-output log))))
             (sleep 0.3)))))
     :name "lem-ratatui debug watchdog")))

(defun make-protocol-stream (fd &key input output)
  "Return an octet stream on FD.

Bytes, not characters: the protocol is binary (lem-relay/protobuf), so
no stream external format, the locale's or any other, stands between the
wire and the codec."
  (sb-sys:make-fd-stream fd
                         :input input
                         :output output
                         :element-type '(unsigned-byte 8)
                         :buffering :full))

(defun call-with-protocol-streams (function)
  "Call FUNCTION with the protocol on its own streams and stdout muffled.

File descriptors 0 and 1 are reopened as octet streams for the wire, and
every stream that would otherwise reach them is redirected.

`*standard-output*' and `*trace-output*' go to a sink, so a stray format
call inside the editor cannot corrupt a frame. `*terminal-io*' and its
derivatives matter just as much and are easier to miss: SBCL's debugger
writes there, not to `*standard-output*', so an unhandled error would
otherwise print its banner straight down the wire. They are pointed at
`*error-output*', which the parent process captures as a log.

The debugger is disabled outright as well. A protocol child has no one to
answer its prompts, and dying on error is the behaviour the display half
wants: it sees EOF and restores the terminal."
  (sb-ext:disable-debugger)
  (setf *log-stream* *error-output*
        *backtrace-delay* (let ((raw (uiop:getenv "LEM_RATATUI_BACKTRACE")))
                            (and raw (ignore-errors (parse-integer raw)))))
  (let* ((sink (make-broadcast-stream))
         (log *error-output*)
         (*protocol-input* (make-protocol-stream 0 :input t))
         (*protocol-output* (make-protocol-stream 1 :output t))
         (*standard-output* sink)
         (*trace-output* sink)
         (*standard-input* (make-concatenated-stream))
         (*terminal-io* (make-two-way-stream (make-concatenated-stream) log))
         (*debug-io* *terminal-io*)
         (*query-io* *terminal-io*))
    (when *backtrace-delay*
      (start-debug-watchdog :delay *backtrace-delay*))
    (funcall function)))
