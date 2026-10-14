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

(defvar *connecting* nil
  "True from one `start' spawning its connect thread until that thread
sets `*agent*' (or gives up); read and set by `start' to refuse a second
call while the first is still in flight. The agent serves one connection
at a time, so two at once would race for its one `Welcome'.")

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
when there is no PATH, a connection is already up, or one started by an
earlier call to this function is still in flight. Until the agent
answers, and if it never does, Lem runs as if there were none.

The agent serves one connection at a time: a second call while connected
would never get a second `Welcome', and the thread it starts would then
set `*agent*' to NIL once it gives up, orphaning the live connection. A
second call is therefore a no-op, not a replacement."
  (when (and path (plusp (length path))
             (not (connected-p))
             (not *connecting*))
    (setf *connecting* t)
    (bt2:make-thread (lambda ()
                       ;; An error escaping this thread would end the
                       ;; process: the debugger is disabled (transport.lisp).
                       (unwind-protect
                            (setf *agent* (handler-case (connect path session-id)
                                            (error (condition)
                                              (log:warn "lem-os-agent: connecting: ~A" condition)
                                              nil)))
                         (setf *connecting* nil)))
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
