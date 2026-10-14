;;;; Channels between Lem and its peer processes (ADR 0019): one package
;;;; for every link to use, made of framing, replies and channel.
(uiop:define-package :lem-ipc-channels
  (:use :cl)
  (:use-reexport :lem-ipc-channels/framing
                 :lem-ipc-channels/replies
                 :lem-ipc-channels/channel))
