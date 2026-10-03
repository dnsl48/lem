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
