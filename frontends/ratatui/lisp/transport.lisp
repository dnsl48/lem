(defpackage :lem-ratatui/transport
  (:use :cl)
  (:export :*protocol-input*
           :*protocol-output*
           :*debug*
           :debug-log
           :*log-stream*
           :start-debug-watchdog
           :*backtrace-delay*
           :stdio-runner
           :call-with-protocol-streams))
(in-package :lem-ratatui/transport)

(defvar *protocol-input* nil
  "The real stdin, held apart from `*standard-input*'.
Bound by `call-with-protocol-streams'.")

(defvar *protocol-output* nil
  "The real stdout, held apart from `*standard-output*'.

Under --mode=stdio the wire and any stray editor output would otherwise
share one stream, and they cannot: muffling `*standard-output*' to
protect the wire would also silence the wire. Keeping the protocol on its
own stream lets `*standard-output*' be redirected to a sink without the
transport noticing.")

(defvar *backtrace-delay* nil
  "Seconds to wait before dumping every thread's backtrace, or NIL.
Set from LEM_RATATUI_BACKTRACE at startup.")

(defvar *debug* nil
  "True when LEM_RATATUI_DEBUG is set in the environment.

Set by `call-with-protocol-streams' at startup rather than here: this
system is delivered through `save-lisp-and-die', so a defvar reading the
environment would capture the *build* machine's setting and bake it into
the image.

Diagnostics go to stderr, which the display half captures as a log;
stdout is the wire and can never be used for this.")

(defvar *log-stream* nil
  "The process's real stderr, captured before anything rebinds it.

`*error-output*' cannot be used directly: most of our code runs on the
editor thread, which `run-editor-thread' wraps in `with-editor-stream',
and that rebinds the standard streams. Diagnostics written there vanish
into the editor rather than reaching the log the display half captures.")

(defun debug-log (control &rest arguments)
  "Write a diagnostic line to the real stderr when `*debug*' is on."
  (when *debug*
    (let ((stream (or *log-stream* *error-output*)))
      (format stream "~&[lem-ratatui] ~?~%" control arguments)
      (force-output stream))))

(defclass stdio-runner (lem-server::server-runner)
  ()
  (:documentation "A stdio server runner that hands the transport explicit streams.

`lem-server::stdio-server-runner' calls `jsonrpc:server-listen' with no
stream arguments, so the transport falls back to its initforms and
captures whatever `*standard-output*' is bound to at that moment — which
is the muffled sink. This runner passes the real streams instead."))

(defmethod lem-server::server-listen ((runner stdio-runner) server)
  "Listen on stdio, giving the transport the protocol streams explicitly.

`jsonrpc:server-listen' forwards unrecognised initargs to the transport's
`make-instance', and `jsonrpc/transport/stdio:stdio-transport' accepts
:input and :output, so this needs no patching of the library."
  (jsonrpc:server-listen server
                         :mode :stdio
                         :input (or *protocol-input* *standard-input*)
                         :output (or *protocol-output* *standard-output*)))

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
  "Return a UTF-8 stream on FD.

The external format is pinned rather than inherited from the locale
because the framing counts bytes: a stream encoding as anything else
would desynchronise every message containing a non-ASCII character."
  (sb-sys:make-fd-stream fd
                         :input input
                         :output output
                         :element-type 'character
                         :external-format :utf-8
                         :buffering :full))

(defun call-with-protocol-streams (function)
  "Call FUNCTION with the protocol on its own streams and stdout muffled.

File descriptors 0 and 1 are reopened as UTF-8 streams for the wire, and
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
        *debug* (and (uiop:getenv "LEM_RATATUI_DEBUG") t)
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
